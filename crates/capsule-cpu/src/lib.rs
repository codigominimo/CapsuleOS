//! # capsule-cpu
//!
//! Modelo de ejecuciÃ³n de la CPU x86-64, registros arquitecturales y decodificador/ejecutor de instrucciones.

pub mod flags;
pub mod modrm;
pub mod registers;

pub use flags::*;
pub use modrm::{ModRm, RexPrefix};
pub use registers::Registers;

use capsule_core::types::GuestAddress;
use capsule_memory::MemoryBus;
use modrm::OperandLocation;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum CpuExitReason {
    Halt,
    IoIn { port: u16, size: u8 },
    IoOut { port: u16, data: u32, size: u8 },
    MmioRead { addr: u64, size: u8 },
    MmioWrite { addr: u64, data: u64, size: u8 },
    Interrupt { vector: u8 },
    Exception { vector: u8, error_code: Option<u32> },
    InvalidOpcode(u8),
    MemoryFault(String),
}

impl fmt::Display for CpuExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpuExitReason::Halt => write!(f, "CPU ejecut? HLT"),
            CpuExitReason::IoIn { port, size } => write!(f, "E/S Lectura Puerto 0x{:04X} ({} bytes)", port, size),
            CpuExitReason::IoOut { port, data, size } => write!(f, "E/S Escritura Puerto 0x{:04X} <- 0x{:X} ({} bytes)", port, data, size),
            CpuExitReason::MmioRead { addr, size } => write!(f, "MMIO Read 0x{addr:016X} ({size} bytes)"),
            CpuExitReason::MmioWrite { addr, data, size } => write!(f, "MMIO Write 0x{addr:016X} <- 0x{data:X} ({size} bytes)"),
            CpuExitReason::Interrupt { vector } => write!(f, "Interrupt vector 0x{vector:02X}"),
            CpuExitReason::Exception { vector, error_code } => write!(f, "Exception 0x{vector:02X} err={error_code:?}"),
            CpuExitReason::InvalidOpcode(op) => write!(f, "Opcode no v?lido: 0x{:02X}", op),
            CpuExitReason::MemoryFault(msg) => write!(f, "Fallo de memoria: {}", msg),
        }
    }
}

/// Procesador virtual emulado x86-64.
pub struct SimpleVcpu {
    pub id: usize,
    pub regs: Registers,
    pub seg_override_base: Option<u64>,
}

fn update_sub_flags_u8_(flags: &mut u64, a: u8, b: u8, r: u8) { *flags &= !(FLAG_CF|FLAG_PF|FLAG_AF|FLAG_ZF|FLAG_SF|FLAG_OF); if (a as u16) < (b as u16) { *flags |= FLAG_CF; } if r==0 { *flags |= FLAG_ZF; } if (r&0x80)!=0 { *flags |= FLAG_SF; } if ((a^b)&(a^r)&0x80)!=0 { *flags |= FLAG_OF; } if ((a^b^r)&0x10)!=0 { *flags |= FLAG_AF; } if (r.count_ones()%2==0) { *flags |= FLAG_PF; } }
impl SimpleVcpu {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            regs: Registers::new(),
            seg_override_base: None,
        }
    }

    /// Empuja un valor de 64 bits en la pila ([RSP - 8] <- val; RSP <- RSP - 8).
    pub fn push_u64(&mut self, mem: &mut dyn MemoryBus, val: u64) -> Result<(), String> {
        self.regs.rsp = self.regs.rsp.wrapping_sub(8);
        let sp = GuestAddress::new(self.regs.rsp);
        mem.write_bytes(sp, &val.to_le_bytes())
            .map_err(|e| format!("Fallo en PUSH u64 a {}: {}", sp, e))
    }

    /// Extrae un valor de 64 bits de la pila (val <- [RSP]; RSP <- RSP + 8).
    pub fn pop_u64(&mut self, mem: &mut dyn MemoryBus) -> Result<u64, String> {
        let sp = GuestAddress::new(self.regs.rsp);
        let val = mem
            .read_u64(sp)
            .map_err(|e| format!("Fallo en POP u64 de {}: {}", sp, e))?;
        self.regs.rsp = self.regs.rsp.wrapping_add(8);
        Ok(val)
    }

    fn push_val(&mut self, mem: &mut dyn MemoryBus, val: u64, size16: bool, size64: bool) -> Result<(), String> {
        if size16 { self.regs.rsp = (self.regs.rsp & !0xFFFF) | ((self.regs.rsp.wrapping_sub(2)) & 0xFFFF); let sp=GuestAddress::new(self.regs.ss_base().wrapping_add(self.regs.rsp&0xFFFF)); mem.write_bytes(sp,&(val as u16).to_le_bytes()).map_err(|e| format!("PUSH16 {}: {}", sp, e)) }
        else if size64 { self.regs.rsp=self.regs.rsp.wrapping_sub(8); let sp=GuestAddress::new(self.regs.rsp); mem.write_bytes(sp,&val.to_le_bytes()).map_err(|e| format!("PUSH64 {}: {}", sp, e)) }
        else { self.regs.rsp=self.regs.rsp.wrapping_sub(4); let sp=GuestAddress::new(self.regs.rsp); mem.write_bytes(sp,&(val as u32).to_le_bytes()).map_err(|e| format!("PUSH32 {}: {}", sp, e)) }
    }
    fn pop_val(&mut self, mem: &mut dyn MemoryBus, size16: bool, size64: bool) -> Result<u64, String> {
        if size16 { let sp=GuestAddress::new(self.regs.ss_base().wrapping_add(self.regs.rsp&0xFFFF)); let v=mem.read_u16(sp).map_err(|e| format!("POP16 {}: {}", sp,e))? as u64; self.regs.rsp=(self.regs.rsp&!0xFFFF)|((self.regs.rsp.wrapping_add(2))&0xFFFF); Ok(v) }
        else if size64 { let sp=GuestAddress::new(self.regs.rsp); let v=mem.read_u64(sp).map_err(|e| format!("POP64 {}: {}", sp,e))?; self.regs.rsp=self.regs.rsp.wrapping_add(8); Ok(v) }
        else { let sp=GuestAddress::new(self.regs.rsp); let v=mem.read_u32(sp).map_err(|e| format!("POP32 {}: {}", sp,e))? as u64; self.regs.rsp=self.regs.rsp.wrapping_add(4); Ok(v) }
    }
    fn alu_op(&mut self, op:u8, dst:u64, src:u64, size16:bool, is_64bit:bool)->u64 {
        let cf= if self.regs.cf(){1u64}else{0};
        match op {
            0=>{ if is_64bit{let r=dst.wrapping_add(src); update_add_flags_u64(&mut self.regs.rflags,dst,src,r); r} else if size16{let r=(dst as u16).wrapping_add(src as u16); update_add_flags_u16(&mut self.regs.rflags,dst as u16,src as u16,r); r as u64} else {let r=(dst as u32).wrapping_add(src as u32); update_add_flags_u32(&mut self.regs.rflags,dst as u32,src as u32,r); r as u64} }
            1=>{ let r=dst|src; if size16{update_logic_flags_u16(&mut self.regs.rflags,r as u16)} else {update_logic_flags_u64(&mut self.regs.rflags,r,is_64bit)} r }
            2=>{ if is_64bit{let r=dst.wrapping_add(src).wrapping_add(cf); update_adc_flags_u64(&mut self.regs.rflags,dst,src,cf,r); r} else if size16{let r=(dst as u16).wrapping_add(src as u16).wrapping_add(cf as u16); update_adc_flags_u16(&mut self.regs.rflags,dst as u16,src as u16,cf as u16,r); r as u64} else {let r=(dst as u32).wrapping_add(src as u32).wrapping_add(cf as u32); update_adc_flags_u32(&mut self.regs.rflags,dst as u32,src as u32,cf as u32,r); r as u64} }
            3=>{ if is_64bit{let r=dst.wrapping_sub(src).wrapping_sub(cf); update_sbb_flags_u64(&mut self.regs.rflags,dst,src,cf,r); r} else if size16{let r=(dst as u16).wrapping_sub(src as u16).wrapping_sub(cf as u16); update_sbb_flags_u16(&mut self.regs.rflags,dst as u16,src as u16,cf as u16,r); r as u64} else {let r=(dst as u32).wrapping_sub(src as u32).wrapping_sub(cf as u32); update_sbb_flags_u32(&mut self.regs.rflags,dst as u32,src as u32,cf as u32,r); r as u64} }
            4=>{ let r=dst&src; if size16{update_logic_flags_u16(&mut self.regs.rflags,r as u16)} else {update_logic_flags_u64(&mut self.regs.rflags,r,is_64bit)} r }
            5=>{ if is_64bit{let r=dst.wrapping_sub(src); update_sub_flags_u64(&mut self.regs.rflags,dst,src,r); r} else if size16{let r=(dst as u16).wrapping_sub(src as u16); update_sub_flags_u16(&mut self.regs.rflags,dst as u16,src as u16,r); r as u64} else {let r=(dst as u32).wrapping_sub(src as u32); update_sub_flags_u32(&mut self.regs.rflags,dst as u32,src as u32,r); r as u64} }
            6=>{ let r=dst^src; if size16{update_logic_flags_u16(&mut self.regs.rflags,r as u16)} else {update_logic_flags_u64(&mut self.regs.rflags,r,is_64bit)} r }
            7=>{ if is_64bit{let r=dst.wrapping_sub(src); update_sub_flags_u64(&mut self.regs.rflags,dst,src,r);} else if size16{let r=(dst as u16).wrapping_sub(src as u16); update_sub_flags_u16(&mut self.regs.rflags,dst as u16,src as u16,r);} else {let r=(dst as u32).wrapping_sub(src as u32); update_sub_flags_u32(&mut self.regs.rflags,dst as u32,src as u32,r);} dst }
            _=>dst
        }
    }

    fn linear_ip(&self) -> u64 { self.regs.cs_base.wrapping_add(self.regs.rip) }
    fn fetch_u16(&mut self, mem: &mut dyn MemoryBus) -> Result<u16, String> {
        let b0 = self.fetch_u8(mem)? as u16;
        let b1 = self.fetch_u8(mem)? as u16;
        Ok(b0 | (b1 << 8))
    }

    fn fetch_u32(&mut self, mem: &mut dyn MemoryBus) -> Result<u32, String> {
        let b0 = self.fetch_u8(mem)? as u32;
        let b1 = self.fetch_u8(mem)? as u32;
        let b2 = self.fetch_u8(mem)? as u32;
        let b3 = self.fetch_u8(mem)? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }
    fn fetch_u64(&mut self, mem: &mut dyn MemoryBus) -> Result<u64, String> { let lo=self.fetch_u32(mem)? as u64; let hi=self.fetch_u32(mem)? as u64; Ok(lo|(hi<<32)) }
    fn fetch_u8(&mut self, mem: &mut dyn MemoryBus) -> Result<u8, String> {
        let linear = self.linear_ip();
        let b = mem.read_u8(GuestAddress::new(linear))
            .map_err(|e| format!("Error leyendo opcode en RIP=0x{:X} CS.base=0x{:X} linear=0x{:X}: {}", self.regs.rip, self.regs.cs_base, linear, e))?;
        self.regs.rip = self.regs.rip.wrapping_add(1);
        Ok(b)
    }

    /// Ejecuta una sola instrucción en memoria física a partir de `regs.rip`.
    pub fn step(&mut self, mem: &mut dyn MemoryBus) -> Result<Option<CpuExitReason>, String> {
        static TRACE_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let trace = std::env::var_os("CAPSULE_TRACE_FW").is_some();
        let rip_before = self.regs.rip;
        let linear_before = self.linear_ip();
        let real_mode = self.regs.real_mode;
        let mut operand_size_16 = real_mode;
        let mut addr_size_16 = real_mode;
        let mut rex = RexPrefix::default();
        self.seg_override_base = None;
        let mut rep_prefix: u8 = 0; // 0 none, 0xF2 REPNZ, 0xF3 REP/REPZ
        let mut opcode = self.fetch_u8(mem)?;
        if trace {
            let n = TRACE_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 20000 { eprintln!("[fwtrace {n:04}] rip=0x{:X} lin=0x{:X} op=0x{:02X} cr0={:X} rsp=0x{:X} cs=0x{:X} csb=0x{:X}", rip_before, linear_before, opcode, self.regs.cr0, self.regs.rsp, self.regs.cs, self.regs.cs_base); }
        }
        loop {
            match opcode {
                0x66 => { operand_size_16 = !operand_size_16; opcode = self.fetch_u8(mem)?; }
                0x67 => { addr_size_16 = !addr_size_16; opcode = self.fetch_u8(mem)?; }
                0x26 => { self.seg_override_base = Some((self.regs.es as u64)<<4); opcode = self.fetch_u8(mem)?; }
                0x2E => { self.seg_override_base = Some(self.regs.cs_base); opcode = self.fetch_u8(mem)?; }
                0x36 => { self.seg_override_base = Some((self.regs.ss as u64)<<4); opcode = self.fetch_u8(mem)?; }
                0x3E => { self.seg_override_base = Some((self.regs.ds as u64)<<4); opcode = self.fetch_u8(mem)?; }
                0x64 => { self.seg_override_base = Some((self.regs.fs as u64)<<4); opcode = self.fetch_u8(mem)?; }
                0x65 => { self.seg_override_base = Some((self.regs.gs as u64)<<4); opcode = self.fetch_u8(mem)?; }
                0xF0 => { opcode = self.fetch_u8(mem)?; }
                0xF2 | 0xF3 => { rep_prefix = opcode; opcode = self.fetch_u8(mem)?; }
                0x40..=0x4F if !real_mode => { rex = RexPrefix::parse(opcode).unwrap_or_default(); opcode = self.fetch_u8(mem)?; }
                _ => break,
            }
        }
        let is_64bit = !real_mode && rex.w;

        if opcode == 0x0F {
            let op2 = self.fetch_u8(mem)?;
            return self.step_0f(op2, &rex, is_64bit, operand_size_16, real_mode, addr_size_16, mem);
        }

        match opcode {
            // CLI/STI - honor IF in software model
            0xFA => { self.regs.interrupts_enabled = false; self.regs.rflags &= !FLAG_IF; Ok(None) }
            0xFB => { self.regs.interrupts_enabled = true; self.regs.rflags |= FLAG_IF; Ok(None) }
            // CLD/STD minimal
            0xF8 => { self.regs.rflags &= !FLAG_CF; Ok(None) }
            0xF9 => { self.regs.rflags |= FLAG_CF; Ok(None) }
            0xF5 => { self.regs.rflags ^= FLAG_CF; Ok(None) }
            0xFC => { self.regs.rflags &= !FLAG_DF; Ok(None) }
            0xFD => { self.regs.rflags |= FLAG_DF; Ok(None) }
            // PUSHA/POPA (real/v86/compat minimal)
            0x60 => {
                let sp = self.regs.rsp;
                if operand_size_16 || real_mode {
                    let vals = [self.regs.rax as u16,self.regs.rcx as u16,self.regs.rdx as u16,self.regs.rbx as u16,sp as u16,self.regs.rbp as u16,self.regs.rsi as u16,self.regs.rdi as u16];
                    for v in vals { self.push_val(mem, v as u64, true, false)?; }
                } else {
                    let vals = [self.regs.eax(),self.regs.ecx(),self.regs.edx(),self.regs.ebx(),self.regs.rsp as u32,self.regs.get_reg32(5),self.regs.get_reg32(6),self.regs.get_reg32(7)];
                    for v in vals { self.push_val(mem, v as u64, false, false)?; }
                }
                Ok(None)
            }
            0x61 => {
                if operand_size_16 || real_mode {
                    let di=self.pop_val(mem,true,false)? as u16; let si=self.pop_val(mem,true,false)? as u16; let bp=self.pop_val(mem,true,false)? as u16; let _=self.pop_val(mem,true,false)?; let bx=self.pop_val(mem,true,false)? as u16; let dx=self.pop_val(mem,true,false)? as u16; let cx=self.pop_val(mem,true,false)? as u16; let ax=self.pop_val(mem,true,false)? as u16;
                    self.regs.rdi=(self.regs.rdi&!0xFFFF)|di as u64; self.regs.rsi=(self.regs.rsi&!0xFFFF)|si as u64; self.regs.rbp=(self.regs.rbp&!0xFFFF)|bp as u64;
                    self.regs.rbx=(self.regs.rbx&!0xFFFF)|bx as u64; self.regs.rdx=(self.regs.rdx&!0xFFFF)|dx as u64; self.regs.rcx=(self.regs.rcx&!0xFFFF)|cx as u64; self.regs.rax=(self.regs.rax&!0xFFFF)|ax as u64;
                } else {
                    let edi=self.pop_val(mem,false,false)? as u32; let esi=self.pop_val(mem,false,false)? as u32; let ebp=self.pop_val(mem,false,false)? as u32; let _=self.pop_val(mem,false,false)?; let ebx=self.pop_val(mem,false,false)? as u32; let edx=self.pop_val(mem,false,false)? as u32; let ecx=self.pop_val(mem,false,false)? as u32; let eax=self.pop_val(mem,false,false)? as u32;
                    self.regs.set_reg32(0,eax); self.regs.set_reg32(1,ecx); self.regs.set_reg32(2,edx); self.regs.set_reg32(3,ebx); self.regs.set_reg32(5,ebp); self.regs.set_reg32(6,esi); self.regs.set_reg32(7,edi);
                }
                Ok(None)
            }

            // MOV r/m16,Sreg (8C) / MOV Sreg,r/m16 (8E)
            0x8C | 0x8E => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                let sreg = modrm.reg & 0x07;
                if opcode == 0x8C {
                    let val = match sreg { 0=>self.regs.es,1=>self.regs.cs,2=>self.regs.ss,3=>self.regs.ds,4=>self.regs.fs,5=>self.regs.gs,_=>0 };
                    rm_loc.write_u16(val, &mut self.regs, mem)?;
                } else {
                    let val = rm_loc.read_u16(&self.regs, mem)?;
                    match sreg {
                        0=>self.regs.es=val, 1=>{self.regs.cs=val; self.regs.cs_base=(val as u64)<<4;}, 2=>self.regs.ss=val, 3=>self.regs.ds=val, 4=>self.regs.fs=val, 5=>self.regs.gs=val,
                        _=>return Ok(Some(CpuExitReason::Exception{vector:6,error_code:None}))
                    }
                }
                Ok(None)
            }

            // INC/DEC r16/r32 (40-4F) real/compat only
            0x40..=0x4F if real_mode => {
                let reg = opcode & 7;
                let size16 = operand_size_16 || real_mode;
                let val = if size16 { self.regs.get_reg16(reg) as u64 } else { self.regs.get_reg32(reg) as u64 };
                let res = if opcode < 0x48 {
                    if size16 { let r=(val as u16).wrapping_add(1); update_add_flags_u16(&mut self.regs.rflags, val as u16,1,r); r as u64 } else { let r=(val as u32).wrapping_add(1); update_add_flags_u32(&mut self.regs.rflags, val as u32,1,r); r as u64 }
                } else {
                    if size16 { let r=(val as u16).wrapping_sub(1); update_sub_flags_u16(&mut self.regs.rflags, val as u16,1,r); r as u64 } else { let r=(val as u32).wrapping_sub(1); update_sub_flags_u32(&mut self.regs.rflags, val as u32,1,r); r as u64 }
                };
                if size16 { self.regs.set_reg16(reg, res as u16); } else { self.regs.set_reg32(reg, res as u32); }
                Ok(None)
            }

            // PUSH/POP segment registers (real/compat)
            0x06 => { self.push_val(mem, self.regs.es as u64, !is_64bit && (operand_size_16||real_mode), is_64bit)?; Ok(None) }
            0x0E => { self.push_val(mem, self.regs.cs as u64, !is_64bit && (operand_size_16||real_mode), is_64bit)?; Ok(None) }
            0x16 => { self.push_val(mem, self.regs.ss as u64, !is_64bit && (operand_size_16||real_mode), is_64bit)?; Ok(None) }
            0x1E => { self.push_val(mem, self.regs.ds as u64, !is_64bit && (operand_size_16||real_mode), is_64bit)?; Ok(None) }
            0x07 => { let v=self.pop_val(mem,!is_64bit&&(operand_size_16||real_mode),is_64bit)? as u16; self.regs.es=v; Ok(None) }
            0x17 => { let v=self.pop_val(mem,!is_64bit&&(operand_size_16||real_mode),is_64bit)? as u16; self.regs.ss=v; Ok(None) }
            0x1F => { let v=self.pop_val(mem,!is_64bit&&(operand_size_16||real_mode),is_64bit)? as u16; self.regs.ds=v; Ok(None) }

            // NOP
            0x90 => Ok(None),

            // HLT - interruptible when IF enabled
            // WAIT/FWAIT
            0x9B => Ok(None),
            // x87 FPU minimal stub (D8-DF)
            0xD8..=0xDF => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                if modrm.mode != 3 { let _rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?; }
                Ok(None)
            }

            // Group 3 F6/F7
            0xF6 | 0xF7 => {
                let is8 = opcode==0xF6;
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let op = modrm.reg & 7;
                if op==0 || op==1 { // TEST imm
                    let imm = if is8 { self.fetch_u8(mem)? as u64 } else if !is_64bit && (operand_size_16||real_mode) { self.fetch_u16(mem)? as u64 } else { self.fetch_u32(mem)? as u64 };
                    let v = if is8 { rm_loc.read_u8(&rex, &self.regs, mem)? as u64 } else if !is_64bit && (operand_size_16||real_mode) { rm_loc.read_u16(&self.regs, mem)? as u64 } else { rm_loc.read_val(is_64bit, &self.regs, mem)? };
                    let res = v & imm; update_logic_flags_u64(&mut self.regs.rflags, res, false); Ok(None)
                } else if op==2 { // NOT
                    if is8 { let v=rm_loc.read_u8(&rex,&self.regs,mem)?; rm_loc.write_u8(!v,&rex,&mut self.regs,mem)?; } else if !is_64bit&&(operand_size_16||real_mode){ let v=rm_loc.read_u16(&self.regs,mem)?; rm_loc.write_u16(!v,&mut self.regs,mem)?; } else { let v=rm_loc.read_val(is_64bit,&self.regs,mem)?; rm_loc.write_val(!v,is_64bit,&mut self.regs,mem)?; } Ok(None)
                } else if op==3 { // NEG
                    if is8 { let v=rm_loc.read_u8(&rex,&self.regs,mem)?; let r=0u8.wrapping_sub(v); update_sub_flags_u8_(&mut self.regs.rflags,0,v,r); rm_loc.write_u8(r,&rex,&mut self.regs,mem)?; }
                    else if !is_64bit&&(operand_size_16||real_mode){ let v=rm_loc.read_u16(&self.regs,mem)?; let r=0u16.wrapping_sub(v); update_sub_flags_u16(&mut self.regs.rflags,0,v,r); rm_loc.write_u16(r,&mut self.regs,mem)?; }
                    else if is_64bit { let v=rm_loc.read_val(true,&self.regs,mem)?; let r=0u64.wrapping_sub(v); update_sub_flags_u64(&mut self.regs.rflags,0,v,r); rm_loc.write_val(r,true,&mut self.regs,mem)?; }
                    else { let v=rm_loc.read_val(false,&self.regs,mem)? as u32; let r=0u32.wrapping_sub(v); update_sub_flags_u32(&mut self.regs.rflags,0,v,r); rm_loc.write_val(r as u64,false,&mut self.regs,mem)?; } Ok(None)
                } else if op==4 || op==5 { // MUL/IMUL
                    if is8 { let a=self.regs.rax as u8 as u64; let b=rm_loc.read_u8(&rex,&self.regs,mem)? as u64; let r=a*b; self.regs.rax=(self.regs.rax&!0xFFFF)| (r&0xFFFF); let c=(r>>8)!=0; if c { self.regs.rflags|=FLAG_CF|FLAG_OF; } else { self.regs.rflags&=!(FLAG_CF|FLAG_OF); } }
                    else if !is_64bit&&(operand_size_16||real_mode){ let a=self.regs.rax as u16 as u64; let b=rm_loc.read_u16(&self.regs,mem)? as u64; let r=a*b; self.regs.rax=(self.regs.rax&!0xFFFF)|(r&0xFFFF); self.regs.rdx=(self.regs.rdx&!0xFFFF)|((r>>16)&0xFFFF); let c=(r>>16)!=0; if c { self.regs.rflags|=FLAG_CF|FLAG_OF; } else { self.regs.rflags&=!(FLAG_CF|FLAG_OF); } }
                    else if is_64bit { let a=self.regs.rax; let b=rm_loc.read_val(true,&self.regs,mem)?; let r=(a as u128)*(b as u128); self.regs.rax=r as u64; self.regs.rdx=(r>>64) as u64; let c=self.regs.rdx!=0; if c { self.regs.rflags|=FLAG_CF|FLAG_OF; } else { self.regs.rflags&=!(FLAG_CF|FLAG_OF); } }
                    else { let a=self.regs.eax() as u64; let b=rm_loc.read_val(false,&self.regs,mem)? as u64; let r=a*b; self.regs.set_reg32(0,r as u32); self.regs.set_reg32(2,(r>>32) as u32); let c=(r>>32)!=0; if c { self.regs.rflags|=FLAG_CF|FLAG_OF; } else { self.regs.rflags&=!(FLAG_CF|FLAG_OF); } } Ok(None)
                } else { // DIV/IDIV minimal unsigned for now
                    if is8 { let d=rm_loc.read_u8(&rex,&self.regs,mem)? as u16; if d==0 { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } let a=self.regs.rax as u16; let q=a/d; let r=a%d; if q>0xFF { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } self.regs.rax=(self.regs.rax&!0xFFFF)|((r as u64)<<8)|(q as u64); }
                    else if !is_64bit&&(operand_size_16||real_mode){ let d=rm_loc.read_u16(&self.regs,mem)? as u32; if d==0 { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } let a=((self.regs.rdx as u32 &0xFFFF)<<16)|(self.regs.rax as u32 &0xFFFF); let q=a/d; let rem=a%d; if q>0xFFFF { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } self.regs.rax=(self.regs.rax&!0xFFFF)|(q as u64); self.regs.rdx=(self.regs.rdx&!0xFFFF)|(rem as u64); }
                    else if is_64bit { let d=rm_loc.read_val(true,&self.regs,mem)? as u128; if d==0 { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } let a=((self.regs.rdx as u128)<<64)|(self.regs.rax as u128); let q=a/d; let rem=a%d; if q>u64::MAX as u128 { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } self.regs.rax=q as u64; self.regs.rdx=rem as u64; }
                    else { let d=rm_loc.read_val(false,&self.regs,mem)? as u64; if d==0 { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } let a=((self.regs.rdx as u64 &0xFFFF_FFFF)<<32)|(self.regs.rax as u64 &0xFFFF_FFFF); let q=a/d; let rem=a%d; if q>0xFFFF_FFFF { return Ok(Some(CpuExitReason::Exception{vector:0,error_code:None})); } self.regs.set_reg32(0,q as u32); self.regs.set_reg32(2,rem as u32); } Ok(None)
                }
            }

            0xF4 => {
                if self.regs.interrupts_enabled {
                    self.regs.rip = self.regs.rip.wrapping_sub(1); // stay on HLT until interrupt
                    Ok(None)
                } else {
                    Ok(Some(CpuExitReason::Halt))
                }
            },

            // MOV r8, imm8 (0xB0..=0xB7)
            0xB0..=0xB7 => {
                let reg_idx = (opcode - 0xB0) + if rex.b { 8 } else { 0 };
                let imm = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo imm8: {}", e))?;
                self.regs.rip += 1;
                self.regs.set_reg8(reg_idx, imm, rex.present);
                Ok(None)
            }

            // ALU AL, imm8: TEST(0xA8), AND(0x24), OR(0x0C), XOR(0x34), CMP(0x3C)
            0xA8 | 0x24 | 0x0C | 0x34 | 0x3C => {
                let imm = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let al = self.regs.get_reg8(0, rex.present);
                match opcode {
                    0xA8 => { // TEST AL, imm8
                        let res = al & imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                    }
                    0x24 => { // AND AL, imm8
                        let res = al & imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                        self.regs.set_reg8(0, res, rex.present);
                    }
                    0x0C => { // OR AL, imm8
                        let res = al | imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                        self.regs.set_reg8(0, res, rex.present);
                    }
                    0x34 => { // XOR AL, imm8
                        let res = al ^ imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                        self.regs.set_reg8(0, res, rex.present);
                    }
                    0x3C => { // CMP AL, imm8
                        let res = al.wrapping_sub(imm);
                        update_sub_flags_u32(&mut self.regs.rflags, al as u32, imm as u32, res as u32);
                    }
                    _ => {}
                }
                Ok(None)
            }

            // CLI/STI
            0xFA => { self.regs.interrupts_enabled = false; self.regs.rflags &= !FLAG_IF; Ok(None) }
            0xFB => { self.regs.interrupts_enabled = true; self.regs.rflags |= FLAG_IF; Ok(None) }
            // CLD/STD (DF not modeled; NOP compat)
            0xFC | 0xFD => Ok(None),

            // INT imm8
            0xCD => {
                let vec = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                self.raise_software_interrupt(vec, mem)?;
                Ok(None)
            }
            // IRET/IRETD/IRETQ - minimal
            0xCF => {
                let size16 = !is_64bit && (operand_size_16 || real_mode);
                let rip = self.pop_val(mem, size16, is_64bit)?;
                let cs = self.pop_val(mem, size16, is_64bit)? as u16;
                let rflags = self.pop_val(mem, size16, is_64bit)?;
                self.regs.cs = cs;
                if self.regs.real_mode || (self.regs.cr0 & 1) == 0 { self.regs.cs_base = (cs as u64) << 4; self.regs.rip = rip & 0xFFFF; } else { self.regs.cs_base = 0; self.regs.rip = rip; }
                self.regs.rflags = (self.regs.rflags & 0xFFFF_0000_0000_0000) | ((rflags | FLAG_RESERVED) & !0x0000_0000_0003_0000);
                self.regs.interrupts_enabled = (self.regs.rflags & FLAG_IF) != 0;
                Ok(None)
            }

            // LIDT/SGDT/etc (0x0F 01 /2 /3 minimal) - handled before this block? no: 0x0F prefix not handled here


            // MOV r32/r64, imm32/imm64 (0xB8..=0xBF)
            0xB8..=0xBF => {
                let reg_idx = (opcode - 0xB8) + if rex.b { 8 } else { 0 };
                if is_64bit {
                    let imm = mem
                        .read_u64(GuestAddress::new(self.regs.rip))
                        .map_err(|e| format!("Error leyendo imm64: {}", e))?;
                    self.regs.rip += 8;
                    self.regs.set_reg64(reg_idx, imm);
                } else {
                    let imm = mem
                        .read_u32(GuestAddress::new(self.regs.rip))
                        .map_err(|e| format!("Error leyendo imm32: {}", e))?;
                    self.regs.rip += 4;
                    self.regs.set_reg32(reg_idx, imm);
                }
                Ok(None)
            }

            // MOV r/m, r (0x89)
            0x89 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                rm_loc.write_val(reg_val, is_64bit, &mut self.regs, mem)?;
                Ok(None)
            }

            // MOV r/m8, r8 (0x88)
            0x88 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = self.regs.get_reg8(modrm.reg, rex.present);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                rm_loc.write_u8(reg_val, &rex, &mut self.regs, mem)?;
                Ok(None)
            }

            // MOV r8, r/m8 (0x8A)
            0x8A => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let byte_val = rm_loc.read_u8(&rex, &self.regs, mem)?;
                self.regs.set_reg8(modrm.reg, byte_val, rex.present);
                Ok(None)
            }

            // Group 2 shifts/rotates: C0/C1/D0/D1/D2/D3
            0xC0|0xC1|0xD0|0xD1|0xD2|0xD3 => {
                let is8b = (opcode & 1) == 0;
                let count = match opcode { 0xD0|0xD1 => 1u32, 0xD2|0xD3 => (self.regs.rcx & 0xFF) as u32, _ => self.fetch_u8(mem)? as u32 };
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                let size16 = !is_64bit && operand_size_16;
                let bits = if is8b { 8u32 } else if is_64bit { 64 } else if size16 { 16 } else { 32 };
                let cnt_mask = if bits==64 { 63 } else { 31 };
                let cnt = count & cnt_mask;
                let val = if is8b { rm_loc.read_u8(&rex, &self.regs, mem)? as u64 } else if size16 { rm_loc.read_u16(&self.regs, mem)? as u64 } else { rm_loc.read_val(is_64bit, &self.regs, mem)? };
                let res = if cnt==0 { val } else { match modrm.reg & 7 {
                    4|6 => { let r=val<<cnt; r }, // SHL/SAL
                    5 => { val >> cnt }, // SHR
                    7 => { if is8b { ((val as i8) >> cnt) as u8 as u64 } else if size16 { ((val as i16) >> cnt) as u16 as u64 } else if is_64bit { ((val as i64) >> cnt) as u64 } else { ((val as i32) >> cnt) as u32 as u64 } },
                    0 => { let c=cnt%bits; if c==0{val}else{(val<<c)|(val>>(bits-c))} }, // ROL
                    1 => { let c=cnt%bits; if c==0{val}else{(val>>c)|(val<<(bits-c))} }, // ROR
                    2 => { let c=cnt%(bits+1); let cf_in=if self.regs.cf(){1u64}else{0}; let mut v=val; let mut cf=cf_in; for _ in 0..c { let new_cf=(v>>(bits-1))&1; v=((v<<1)|cf) & if bits==64{!0u64}else{(1u64<<bits)-1}; cf=new_cf; } if c>0 { if cf==1{self.regs.rflags|=FLAG_CF;} else {self.regs.rflags&=!FLAG_CF;} } v }, // RCL simplified
                    3 => { let c=cnt%(bits+1); let cf_in=if self.regs.cf(){1u64}else{0}; let mut v=val; let mut cf=cf_in; for _ in 0..c { let new_cf=v&1; v=(v>>1)|(cf<<(bits-1)); cf=new_cf; } if c>0 { if cf==1{self.regs.rflags|=FLAG_CF;} else {self.regs.rflags&=!FLAG_CF;} } v }, // RCR simplified
                    _ => val,
                }};
                let mask = if bits==64 { !0u64 } else { (1u64<<bits)-1 };
                let res = res & mask;
                if is8b { rm_loc.write_u8(res as u8, &rex, &mut self.regs, mem)?; }
                else if size16 { rm_loc.write_u16(res as u16, &mut self.regs, mem)?; }
                else { rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?; }
                if cnt!=0 && (modrm.reg & 7) != 2 && (modrm.reg & 7) != 3 {
                    let cf = match modrm.reg & 7 { 4|6|0 => (val >> (bits-cnt)) & 1, 5|1|7 => (val >> (cnt-1)) & 1, _ => 0 };
                    if cf==1 { self.regs.rflags|=FLAG_CF; } else { self.regs.rflags&=!FLAG_CF; }
                    let sign = (res >> (bits-1)) & 1;
                    if res==0 { self.regs.rflags|=FLAG_ZF; } else { self.regs.rflags&=!FLAG_ZF; }
                    if sign==1 { self.regs.rflags|=FLAG_SF; } else { self.regs.rflags&=!FLAG_SF; }
                    if calculate_parity((res&0xFF) as u8) { self.regs.rflags|=FLAG_PF; } else { self.regs.rflags&=!FLAG_PF; }
                }
                Ok(None)
            }

            // Grupo 1 r/m8, imm8 (0x80): /0 ADD, /1 OR, /4 AND, /5 SUB, /6 XOR, /7 CMP
            0x80 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let dst = rm_loc.read_u8(&rex, &self.regs, mem)?;
                let imm = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                match modrm.reg & 0x07 {
                    0 => { let res = dst.wrapping_add(imm); update_add_flags_u32(&mut self.regs.rflags, dst as u32, imm as u32, res as u32); rm_loc.write_u8(res, &rex, &mut self.regs, mem)?; }
                    1 => { let res = dst | imm; update_logic_flags_u64(&mut self.regs.rflags, res as u64, false); rm_loc.write_u8(res, &rex, &mut self.regs, mem)?; }
                    4 => { let res = dst & imm; update_logic_flags_u64(&mut self.regs.rflags, res as u64, false); rm_loc.write_u8(res, &rex, &mut self.regs, mem)?; }
                    5 => { let res = dst.wrapping_sub(imm); update_sub_flags_u32(&mut self.regs.rflags, dst as u32, imm as u32, res as u32); rm_loc.write_u8(res, &rex, &mut self.regs, mem)?; }
                    6 => { let res = dst ^ imm; update_logic_flags_u64(&mut self.regs.rflags, res as u64, false); rm_loc.write_u8(res, &rex, &mut self.regs, mem)?; }
                    7 => { let res = dst.wrapping_sub(imm); update_sub_flags_u32(&mut self.regs.rflags, dst as u32, imm as u32, res as u32); }
                    other => return Err(format!("Operacion grupo 0x80 /{} no soportada", other)),
                }
                Ok(None)
            }

            // TEST r/m8, r8 (0x84)
            0x84 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = self.regs.get_reg8(modrm.reg, rex.present);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let rm_val = rm_loc.read_u8(&rex, &self.regs, mem)?;
                let res = rm_val & reg_val;
                update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                Ok(None)
            }

            // MOVS/STOS/LODS/CMPS/SCAS (A4-A7, AA-AF) minimal, REP support
            0xA4..=0xA7 | 0xAA..=0xAF => {
                let size = if opcode==0xA4||opcode==0xA6||opcode==0xAA||opcode==0xAC||opcode==0xAE { 1u64 } else if operand_size_16 { 2u64 } else if is_64bit { 8u64 } else { 4u64 };
                let count_reg = if addr_size_16 { self.regs.get_reg16(1) as u64 } else if is_64bit { self.regs.rcx } else { self.regs.get_reg32(1) as u64 };
                let mut count = if rep_prefix!=0 { count_reg } else { 1 };
                while count>0 {
                    let si = if addr_size_16 { self.regs.get_reg16(6) as u64 } else if is_64bit { self.regs.rsi } else { self.regs.get_reg32(6) as u64 };
                    let di = if addr_size_16 { self.regs.get_reg16(7) as u64 } else if is_64bit { self.regs.rdi } else { self.regs.get_reg32(7) as u64 };
                    let ds_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                    let es_base = if real_mode { (self.regs.es as u64)<<4 } else { 0 };
                    let df = (self.regs.rflags & FLAG_DF)!=0; let step = if df { (0u64).wrapping_sub(size) } else { size };
                    match opcode {
                        0xA4|0xA5 => { let mut b=[0u8;8]; let n=size as usize; mem.read_bytes(GuestAddress::new(ds_base.wrapping_add(si)), &mut b[..n]).map_err(|e|e.to_string())?; mem.write_bytes(GuestAddress::new(es_base.wrapping_add(di)), &b[..n]).map_err(|e|e.to_string())?; self.regs.rsi = if addr_size_16 { (self.regs.rsi&!0xFFFF)|((si.wrapping_add(step)&0xFFFF)) } else if is_64bit { si.wrapping_add(step) } else { (self.regs.rsi&!0xFFFF_FFFF)|((si as u32).wrapping_add(step as u32) as u64)}; self.regs.rdi = if addr_size_16 { (self.regs.rdi&!0xFFFF)|((di.wrapping_add(step)&0xFFFF)) } else if is_64bit { di.wrapping_add(step) } else { (self.regs.rdi&!0xFFFF_FFFF)|((di as u32).wrapping_add(step as u32) as u64)}; }
                        0xAA|0xAB => { let mut b=[0u8;8]; let n=size as usize; b[..n].copy_from_slice(&self.regs.rax.to_le_bytes()[..n]); mem.write_bytes(GuestAddress::new(es_base.wrapping_add(di)), &b[..n]).map_err(|e|e.to_string())?; self.regs.rdi = if addr_size_16 { (self.regs.rdi&!0xFFFF)|((di.wrapping_add(step)&0xFFFF)) } else if is_64bit { di.wrapping_add(step) } else { (self.regs.rdi&!0xFFFF_FFFF)|((di as u32).wrapping_add(step as u32) as u64)}; }
                        0xAC|0xAD => { let mut b=[0u8;8]; let n=size as usize; mem.read_bytes(GuestAddress::new(ds_base.wrapping_add(si)), &mut b[..n]).map_err(|e|e.to_string())?; if size==1 { self.regs.rax=(self.regs.rax&!0xFF)|(b[0] as u64);} else if size==2 { self.regs.rax=(self.regs.rax&!0xFFFF)|(u16::from_le_bytes([b[0],b[1]]) as u64);} else if size==4 { self.regs.set_reg32(0,u32::from_le_bytes([b[0],b[1],b[2],b[3]])); } else { self.regs.rax=u64::from_le_bytes(b); } self.regs.rsi = if addr_size_16 { (self.regs.rsi&!0xFFFF)|((si.wrapping_add(step)&0xFFFF)) } else if is_64bit { si.wrapping_add(step) } else { (self.regs.rsi&!0xFFFF_FFFF)|((si as u32).wrapping_add(step as u32) as u64)}; }
                        0xA6|0xA7|0xAE|0xAF => { let mut sb=[0u8;8]; let mut db=[0u8;8]; let n=size as usize; if opcode==0xAE||opcode==0xAF { db[..n].copy_from_slice(&self.regs.rax.to_le_bytes()[..n]); } else { mem.read_bytes(GuestAddress::new(ds_base.wrapping_add(si)), &mut sb[..n]).map_err(|e|e.to_string())?; } let dv = { mem.read_bytes(GuestAddress::new(es_base.wrapping_add(di)), &mut db[..n]).map_err(|e|e.to_string())?; u64::from_le_bytes(db) & (if size==8 {!0} else {(1u64<<(size*8))-1}) }; let sv = if opcode==0xAE||opcode==0xAF { self.regs.rax & (if size==8 {!0} else {(1u64<<(size*8))-1}) } else { u64::from_le_bytes(sb) & (if size==8 {!0} else {(1u64<<(size*8))-1}) }; let r = sv.wrapping_sub(dv); if size==1 { { let r8=r as u8; let f=&mut self.regs.rflags; *f&=!(FLAG_CF|FLAG_PF|FLAG_AF|FLAG_ZF|FLAG_SF|FLAG_OF); if (sv as u8)<(dv as u8){*f|=FLAG_CF;} if r8==0{*f|=FLAG_ZF;} if (r8&0x80)!=0{*f|=FLAG_SF;} if ((sv^dv)&(sv^(r8 as u64))&0x80)!=0{*f|=FLAG_OF;} if (((sv as u8)^(dv as u8)^(r8))&0x10)!=0{*f|=FLAG_AF;} if (r8.count_ones()%2==0){*f|=FLAG_PF;} }} else if size==2 { update_sub_flags_u16(&mut self.regs.rflags, sv as u16, dv as u16, r as u16);} else if size==4 { update_sub_flags_u32(&mut self.regs.rflags, sv as u32, dv as u32, r as u32);} else { update_sub_flags_u64(&mut self.regs.rflags, sv,dv,r);} if opcode!=0xAE && opcode!=0xAF { self.regs.rsi = if addr_size_16 { (self.regs.rsi&!0xFFFF)|((si.wrapping_add(step)&0xFFFF)) } else if is_64bit { si.wrapping_add(step) } else { (self.regs.rsi&!0xFFFF_FFFF)|((si as u32).wrapping_add(step as u32) as u64)}; } self.regs.rdi = if addr_size_16 { (self.regs.rdi&!0xFFFF)|((di.wrapping_add(step)&0xFFFF)) } else if is_64bit { di.wrapping_add(step) } else { (self.regs.rdi&!0xFFFF_FFFF)|((di as u32).wrapping_add(step as u32) as u64)}; if rep_prefix==0xF3 && !self.regs.zf() { break; } if rep_prefix==0xF2 && self.regs.zf() { break; } }
                        _=>{}
                    }
                    if rep_prefix!=0 { count-=1; if count==0 { break; } if opcode>=0xA6 && opcode<=0xA7 { } if opcode>=0xAE { } } else { break; }
                }
                if rep_prefix!=0 { if addr_size_16 { self.regs.rcx=(self.regs.rcx&!0xFFFF)|count; } else if is_64bit { self.regs.rcx=count; } else { self.regs.set_reg32(1,count as u32);} }
                Ok(None)
            }

            // MOV AL/AX/EAX/RAX, moffs (A0-A3)
            0xA0..=0xA3 => {
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let addr = if real_mode || addr_size_16 { seg.wrapping_add(self.fetch_u16(mem)? as u64) } else { seg.wrapping_add(if is_64bit { self.fetch_u64(mem)? } else { self.fetch_u32(mem)? as u64 }) };
                let a = GuestAddress::new(addr);
                match opcode {
                    0xA0 => { self.regs.rax = (self.regs.rax & !0xFF) | (mem.read_u8(a).map_err(|e|e.to_string())? as u64); }
                    0xA1 => { if is_64bit { let mut b=[0u8;8]; mem.read_bytes(a,&mut b).map_err(|e|e.to_string())?; self.regs.rax=u64::from_le_bytes(b); } else if operand_size_16 { let mut b=[0u8;2]; mem.read_bytes(a,&mut b).map_err(|e|e.to_string())?; self.regs.rax=(self.regs.rax&!0xFFFF)|(u16::from_le_bytes(b) as u64); } else { let mut b=[0u8;4]; mem.read_bytes(a,&mut b).map_err(|e|e.to_string())?; self.regs.set_reg32(0,u32::from_le_bytes(b)); } }
                    0xA2 => { mem.write_bytes(a,&[(self.regs.rax & 0xFF) as u8]).map_err(|e|e.to_string())?; }
                    0xA3 => { if is_64bit { mem.write_bytes(a,&self.regs.rax.to_le_bytes()).map_err(|e|e.to_string())?; } else if operand_size_16 { mem.write_bytes(a,&((self.regs.rax&0xFFFF) as u16).to_le_bytes()).map_err(|e|e.to_string())?; } else { mem.write_bytes(a,&self.regs.eax().to_le_bytes()).map_err(|e|e.to_string())?; } }
                    _=>{}
                }
                Ok(None)
            }

            // LEA r, m (0x8D)
            0x8D => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let addr = rm_loc.addr().ok_or_else(|| "LEA requires memory operand".to_string())?.0;
                let ea = if real_mode || addr_size_16 { addr.wrapping_sub(seg_base) & 0xFFFF } else { addr.wrapping_sub(seg_base) };
                if is_64bit { self.regs.set_reg64(modrm.reg, ea); } else if operand_size_16 || real_mode { self.regs.set_reg16(modrm.reg, ea as u16); } else { self.regs.set_reg32(modrm.reg, ea as u32); }
                Ok(None)
            }

            // MOV r, r/m (0x8B)
            0x8B => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let val = rm_loc.read_val(is_64bit, &self.regs, mem)?;
                if is_64bit {
                    self.regs.set_reg64(modrm.reg, val);
                } else {
                    self.regs.set_reg32(modrm.reg, val as u32);
                }
                Ok(None)
            }

            // MOV r/m, imm16/32 (0xC7 /0)
            0xC7 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                if (modrm.reg & 7) != 0 { return Ok(Some(CpuExitReason::Exception { vector: 6, error_code: None })); }
                let size16 = !is_64bit && (operand_size_16 || real_mode);
                if size16 { let imm = self.fetch_u16(mem)? as u64; rm_loc.write_u16(imm as u16, &mut self.regs, mem)?; }
                else if is_64bit { let imm = self.fetch_u32(mem)? as i32 as i64 as u64; rm_loc.write_val(imm, true, &mut self.regs, mem)?; }
                else { let imm = self.fetch_u32(mem)? as u64; rm_loc.write_val(imm, false, &mut self.regs, mem)?; }
                Ok(None)
            }

            // ALU general: ADD/OR/ADC/SBB/AND/SUB/XOR/CMP (00-3D)
            0x00..=0x05 | 0x08..=0x0D | 0x10..=0x15 | 0x18..=0x1D | 0x20..=0x25 | 0x28..=0x2D | 0x30..=0x35 | 0x38..=0x3D => {
                let op = (opcode >> 3) & 0x07;
                let dir = (opcode & 0x02) != 0;
                let is8 = (opcode & 0x01) == 0;
                let size16 = !is_64bit && operand_size_16;
                if (opcode & 0x04) == 0x04 {
                    let imm = if is8 { self.fetch_u8(mem)? as u64 }
                        else if is_64bit { self.fetch_u32(mem)? as i32 as i64 as u64 }
                        else if size16 { self.fetch_u16(mem)? as u64 }
                        else { self.fetch_u32(mem)? as u64 };
                    let dst = if is8 { self.regs.rax & 0xFF } else if is_64bit { self.regs.rax } else if size16 { self.regs.rax & 0xFFFF } else { self.regs.eax() as u64 };
                    let res = self.alu_op(op, dst, imm, size16, is_64bit);
                    if op != 7 {
                        if is8 { self.regs.rax = (self.regs.rax & !0xFF) | (res & 0xFF); }
                        else if is_64bit { self.regs.rax = res; }
                        else if size16 { self.regs.rax = (self.regs.rax & !0xFFFF) | (res & 0xFFFF); }
                        else { self.regs.set_reg32(0, res as u32); }
                    }
                    return Ok(None);
                }
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                if is8 {
                    let rmv = rm_loc.read_u8(&rex, &self.regs, mem)? as u64;
                    let rv = self.regs.get_reg8(modrm.reg, rex.present) as u64;
                    let (dst, src) = if dir { (rv, rmv) } else { (rmv, rv) };
                    let res = self.alu_op(op, dst, src, false, false) & 0xFF;
                    if op != 7 {
                        if dir { self.regs.set_reg8(modrm.reg, res as u8, rex.present); }
                        else { rm_loc.write_u8(res as u8, &rex, &mut self.regs, mem)?; }
                    }
                } else {
                    let rmv = if size16 { rm_loc.read_u16(&self.regs, mem)? as u64 } else { rm_loc.read_val(is_64bit, &self.regs, mem)? };
                    let rv = if is_64bit { self.regs.get_reg64(modrm.reg) } else if size16 { self.regs.get_reg16(modrm.reg) as u64 } else { self.regs.get_reg32(modrm.reg) as u64 };
                    let (dst, src) = if dir { (rv, rmv) } else { (rmv, rv) };
                    let res = self.alu_op(op, dst, src, size16, is_64bit);
                    if op != 7 {
                        if dir {
                            if is_64bit { self.regs.set_reg64(modrm.reg, res); }
                            else if size16 { self.regs.set_reg16(modrm.reg, res as u16); }
                            else { self.regs.set_reg32(modrm.reg, res as u32); }
                        } else {
                            if size16 { rm_loc.write_u16(res as u16, &mut self.regs, mem)?; }
                            else { rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?; }
                        }
                    }
                }
                Ok(None)
            }

            // Grupo 1: ADD, OR, AND, SUB, XOR, CMP con imm32 (0x81) o imm8 (0x83)
            0x81 | 0x83 => {
                let is_imm8 = opcode == 0x83;
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let size16 = !is_64bit && operand_size_16;
                let dst_val = if is_64bit { rm_loc.read_val(true, &self.regs, mem)? }
                    else if size16 { rm_loc.read_u16(&self.regs, mem)? as u64 }
                    else { rm_loc.read_val(false, &self.regs, mem)? };

                let imm: u64 = if is_imm8 {
                    let imm8 = self.fetch_u8(mem)? as i8;
                    imm8 as i32 as u32 as u64
                } else if size16 {
                    let imm16 = self.fetch_u16(mem)? as i16;
                    imm16 as i32 as u32 as u64
                } else {
                    let imm32 = self.fetch_u32(mem)?;
                    if is_64bit { imm32 as i32 as i64 as u64 } else { imm32 as u64 }
                };

                match modrm.reg & 0x07 {
                    0 => { // ADD
                        if is_64bit { let res=dst_val.wrapping_add(imm); update_add_flags_u64(&mut self.regs.rflags,dst_val,imm,res); rm_loc.write_val(res,true,&mut self.regs,mem)?; }
                        else if size16 { let res=(dst_val as u16).wrapping_add(imm as u16); update_add_flags_u16(&mut self.regs.rflags,dst_val as u16,imm as u16,res); rm_loc.write_u16(res,&mut self.regs,mem)?; }
                        else { let res=(dst_val as u32).wrapping_add(imm as u32); update_add_flags_u32(&mut self.regs.rflags,dst_val as u32,imm as u32,res); rm_loc.write_val(res as u64,false,&mut self.regs,mem)?; }
                    }
                    1 => { let res=dst_val|imm;  if is_64bit { update_logic_flags_u64(&mut self.regs.rflags,res,true); rm_loc.write_val(res,true,&mut self.regs,mem)?; } else if size16 { update_logic_flags_u16(&mut self.regs.rflags,res as u16); rm_loc.write_u16(res as u16,&mut self.regs,mem)?; } else { update_logic_flags_u64(&mut self.regs.rflags,res,false); rm_loc.write_val(res as u64,false,&mut self.regs,mem)?; }  }
                    4 => { let res=dst_val&imm; if is_64bit { update_logic_flags_u64(&mut self.regs.rflags,res,true); rm_loc.write_val(res,true,&mut self.regs,mem)?; } else if size16 { update_logic_flags_u16(&mut self.regs.rflags,res as u16); rm_loc.write_u16(res as u16,&mut self.regs,mem)?; } else { update_logic_flags_u64(&mut self.regs.rflags,res,false); rm_loc.write_val(res as u64,false,&mut self.regs,mem)?; } }
                    5 => { // SUB
                        if is_64bit { let res=dst_val.wrapping_sub(imm); update_sub_flags_u64(&mut self.regs.rflags,dst_val,imm,res); rm_loc.write_val(res,true,&mut self.regs,mem)?; }
                        else if size16 { let res=(dst_val as u16).wrapping_sub(imm as u16); update_sub_flags_u16(&mut self.regs.rflags,dst_val as u16,imm as u16,res); rm_loc.write_u16(res,&mut self.regs,mem)?; }
                        else { let res=(dst_val as u32).wrapping_sub(imm as u32); update_sub_flags_u32(&mut self.regs.rflags,dst_val as u32,imm as u32,res); rm_loc.write_val(res as u64,false,&mut self.regs,mem)?; }
                    }
                    6 => { let res=dst_val^imm; if is_64bit { update_logic_flags_u64(&mut self.regs.rflags,res,true); rm_loc.write_val(res,true,&mut self.regs,mem)?; } else if size16 { update_logic_flags_u16(&mut self.regs.rflags,res as u16); rm_loc.write_u16(res as u16,&mut self.regs,mem)?; } else { update_logic_flags_u64(&mut self.regs.rflags,res,false); rm_loc.write_val(res as u64,false,&mut self.regs,mem)?; } }
                    7 => { // CMP
                        if is_64bit { let res=dst_val.wrapping_sub(imm); update_sub_flags_u64(&mut self.regs.rflags,dst_val,imm,res); }
                        else if size16 { let res=(dst_val as u16).wrapping_sub(imm as u16); update_sub_flags_u16(&mut self.regs.rflags,dst_val as u16,imm as u16,res); }
                        else { let res=(dst_val as u32).wrapping_sub(imm as u32); update_sub_flags_u32(&mut self.regs.rflags,dst_val as u32,imm as u32,res); }
                    }
                    other => return Err(format!("Operación grupo 0x81/0x83 /{} no soportada", other)),
                }
                Ok(None)
            }

            // SUB r/m, r (0x29)
            0x29 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;

                if is_64bit {
                    let res = dst_val.wrapping_sub(reg_val);
                    update_sub_flags_u64(&mut self.regs.rflags, dst_val, reg_val, res);
                    rm_loc.write_val(res, true, &mut self.regs, mem)?;
                } else {
                    let res = (dst_val as u32).wrapping_sub(reg_val as u32);
                    update_sub_flags_u32(&mut self.regs.rflags, dst_val as u32, reg_val as u32, res);
                    rm_loc.write_val(res as u64, false, &mut self.regs, mem)?;
                }
                Ok(None)
            }

            // CMP r/m, r (0x39)
            0x39 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;

                if is_64bit {
                    let res = dst_val.wrapping_sub(reg_val);
                    update_sub_flags_u64(&mut self.regs.rflags, dst_val, reg_val, res);
                } else {
                    let res = (dst_val as u32).wrapping_sub(reg_val as u32);
                    update_sub_flags_u32(&mut self.regs.rflags, dst_val as u32, reg_val as u32, res);
                }
                Ok(None)
            }

            // XOR r/m, r (0x31)
            0x31 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;
                let res = dst_val ^ reg_val;
                update_logic_flags_u64(&mut self.regs.rflags, res, is_64bit);
                rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?;
                Ok(None)
            }

            // TEST r/m, r (0x85)
            0x85 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;
                let res = dst_val & reg_val;
                update_logic_flags_u64(&mut self.regs.rflags, res, is_64bit);
                Ok(None)
            }

            // Grupo 4: INC / DEC (0xFF /0 o /1)
            0xFF => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 }); let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, &rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                let val = rm_loc.read_val(is_64bit, &self.regs, mem)?;

                match modrm.reg & 0x07 {
                    0 => {
                        // INC
                        let res = val.wrapping_add(1);
                        if is_64bit {
                            update_add_flags_u64(&mut self.regs.rflags, val, 1, res);
                            rm_loc.write_val(res, true, &mut self.regs, mem)?;
                        } else {
                            let res32 = (val as u32).wrapping_add(1);
                            update_add_flags_u32(&mut self.regs.rflags, val as u32, 1, res32);
                            rm_loc.write_val(res32 as u64, false, &mut self.regs, mem)?;
                        }
                    }
                    1 => {
                        // DEC
                        let res = val.wrapping_sub(1);
                        if is_64bit {
                            update_sub_flags_u64(&mut self.regs.rflags, val, 1, res);
                            rm_loc.write_val(res, true, &mut self.regs, mem)?;
                        } else {
                            let res32 = (val as u32).wrapping_sub(1);
                            update_sub_flags_u32(&mut self.regs.rflags, val as u32, 1, res32);
                            rm_loc.write_val(res32 as u64, false, &mut self.regs, mem)?;
                        }
                    }
                    2 => {
                        // CALL r/m (near, indirect)
                        let ret_addr = self.regs.rip;
                        { let size16=!is_64bit && (operand_size_16 || real_mode); self.push_val(mem, ret_addr, size16, is_64bit)?; }
                        self.regs.rip = val;
                    }
                    4 => {
                        // JMP r/m (near, indirect)
                        self.regs.rip = val;
                    }
                    other => return Err(format!("OperaciÃ³n 0xFF /{} no implementada", other)),
                }
                Ok(None)
            }

            // PUSH r64 (0x50..=0x57)
            0x50..=0x57 => {
                let reg_idx = (opcode - 0x50) + if rex.b { 8 } else { 0 };
                let val = self.regs.get_reg64(reg_idx);
                { let size16=!is_64bit && (operand_size_16 || real_mode); self.push_val(mem, val, size16, is_64bit)?; }
                Ok(None)
            }

            // POP r64 (0x58..=0x5F)
            0x58..=0x5F => {
                let reg_idx = (opcode - 0x58) + if rex.b { 8 } else { 0 };
                let size16=!is_64bit && (operand_size_16 || real_mode); let val = self.pop_val(mem, size16, is_64bit)?;
                self.regs.set_reg64(reg_idx, val);
                Ok(None)
            }

            // PUSHFQ (0x9C)
            0x9C => {
                let val = self.regs.rflags | FLAG_RESERVED;
                { let size16=!is_64bit && (operand_size_16 || real_mode); self.push_val(mem, val, size16, is_64bit)?; }
                Ok(None)
            }
            // POPFQ (0x9D)
            0x9D => {
                let size16=!is_64bit && (operand_size_16 || real_mode); let val = self.pop_val(mem, size16, is_64bit)?;
                self.regs.rflags = (val | FLAG_RESERVED) & !0x0000_0000_0003_0000; // keep RF/VM cleared in minimal model
                self.regs.interrupts_enabled = (self.regs.rflags & FLAG_IF) != 0;
                Ok(None)
            }

            // JMP rel8 (0xEB)
            0xEB => {
                let rel8 = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i8;
                self.regs.rip += 1;
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel8 as i64) as u64;
                Ok(None)
            }

            // JMP rel16/rel32 (0xE9)
            0xE9 => {
                let rel = if !is_64bit && (operand_size_16 || real_mode) { self.fetch_u16(mem)? as i16 as i64 } else { self.fetch_u32(mem)? as i32 as i64 };
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel) as u64;
                Ok(None)
            }

            // JMP ptr16:16/ptr16:32 (0xEA) - real/protected mode far jump, minimal
            0xEA => {
                let off = if operand_size_16 {
                    let lo = self.fetch_u8(mem)? as u64; let hi = self.fetch_u8(mem)? as u64; lo | (hi<<8)
                } else {
                    let b0 = self.fetch_u8(mem)? as u64; let b1 = self.fetch_u8(mem)? as u64;
                    let b2 = self.fetch_u8(mem)? as u64; let b3 = self.fetch_u8(mem)? as u64;
                    b0 | (b1<<8) | (b2<<16) | (b3<<24)
                };
                let slo = self.fetch_u8(mem)? as u16; let shi = self.fetch_u8(mem)? as u16; let sel = slo | (shi<<8);
                self.regs.cs = sel;
                if self.regs.real_mode { if operand_size_16 { self.regs.cs_base = (sel as u64) << 4; self.regs.rip = off & 0xFFFF; } else { self.regs.cs_base = 0; self.regs.rip = off; } } else { self.regs.cs_base = 0; self.regs.rip = off; }
                Ok(None)
            }

            // CALL rel32 (0xE8)
            0xE8 => {
                let rel32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i32;
                self.regs.rip += 4;
                // Empujar la direcciÃ³n de retorno en la pila
                let ret_addr = self.regs.rip;
                { let size16=!is_64bit && (operand_size_16 || real_mode); self.push_val(mem, ret_addr, size16, is_64bit)?; }
                // Saltar al destino
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel32 as i64) as u64;
                Ok(None)
            }

            // RET (0xC3)
            0xC3 => {
                let size16=!is_64bit && (operand_size_16 || real_mode); let ret_addr = self.pop_val(mem, size16, is_64bit)?;
                self.regs.rip = ret_addr;
                Ok(None)
            }

            // Saltos condicionales Jcc rel8 (0x70..=0x7F)
            0x70..=0x7F => {
                let rel8 = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i8;
                self.regs.rip += 1;

                let condition_met = match opcode {
                    0x70 => self.regs.of(),                 // JO
                    0x71 => !self.regs.of(),                // JNO
                    0x72 => self.regs.cf(),                 // JB / JC
                    0x73 => !self.regs.cf(),                // JAE / JNC
                    0x74 => self.regs.zf(),                 // JZ / JE
                    0x75 => !self.regs.zf(),                // JNZ / JNE
                    0x76 => self.regs.cf() || self.regs.zf(), // JBE
                    0x77 => !self.regs.cf() && !self.regs.zf(),// JA
                    0x78 => self.regs.sf(),                 // JS
                    0x79 => !self.regs.sf(),                // JNS
                    0x7C => self.regs.sf() != self.regs.of(), // JL
                    0x7D => self.regs.sf() == self.regs.of(), // JGE
                    0x7E => self.regs.zf() || (self.regs.sf() != self.regs.of()), // JLE
                    0x7F => !self.regs.zf() && (self.regs.sf() == self.regs.of()), // JG
                    _ => false,
                };

                if condition_met {
                    self.regs.rip = (self.regs.rip as i64).wrapping_add(rel8 as i64) as u64;
                }
                Ok(None)
            }

            // IN AL, imm8 (0xE4)
            0xE4 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                Ok(Some(CpuExitReason::IoIn { port, size: 1 }))
            }

            // IN AX/EAX, imm8 (0xE5) - 0x66 selecciona AX
            0xE5 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                let size = if operand_size_16 { 2 } else { 4 };
                Ok(Some(CpuExitReason::IoIn { port, size }))
            }

            // OUT imm8, AL (0xE6)
            0xE6 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                let data = (self.regs.rax & 0xFF) as u32;
                Ok(Some(CpuExitReason::IoOut { port, data, size: 1 }))
            }

            // OUT imm8, AX/EAX (0xE7) - 0x66 selecciona AX
            0xE7 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                if operand_size_16 {
                    let data = (self.regs.rax & 0xFFFF) as u32;
                    Ok(Some(CpuExitReason::IoOut { port, data, size: 2 }))
                } else {
                    let data = self.regs.eax();
                    Ok(Some(CpuExitReason::IoOut { port, data, size: 4 }))
                }
            }

            // IN AL, DX (0xEC)
            0xEC => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                Ok(Some(CpuExitReason::IoIn { port, size: 1 }))
            }

            // IN AX/EAX, DX (0xED) - 0x66 selecciona AX
            0xED => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                let size = if operand_size_16 { 2 } else { 4 };
                Ok(Some(CpuExitReason::IoIn { port, size }))
            }

            // OUT DX, AL (0xEE)
            0xEE => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                let data = (self.regs.rax & 0xFF) as u32;
                Ok(Some(CpuExitReason::IoOut { port, data, size: 1 }))
            }

            // OUT DX, AX/EAX (0xEF) - 0x66 selecciona AX
            0xEF => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                if operand_size_16 {
                    let data = (self.regs.rax & 0xFFFF) as u32;
                    Ok(Some(CpuExitReason::IoOut { port, data, size: 2 }))
                } else {
                    let data = self.regs.eax();
                    Ok(Some(CpuExitReason::IoOut { port, data, size: 4 }))
                }
            }

            other => Ok(Some(CpuExitReason::Exception { vector: 6, error_code: None /* #UD */ })),
        }
    }
}

impl SimpleVcpu {
    fn step_0f(&mut self, op2: u8, rex: &RexPrefix, is_64bit: bool, operand_size_16: bool, real_mode: bool, addr_size_16: bool, mem: &mut dyn MemoryBus) -> Result<Option<CpuExitReason>, String> {
        match op2 {
            // LIDT / SIDT (0F 01 /2 /3) - minimal support
            0x01 => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, rex);
                let seg_base = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, rex, &mut self.regs, mem, real_mode || addr_size_16, seg_base)?;
                match modrm.reg & 0x07 {
                    2 => { // LGDT
                        let addr = match rm_loc { OperandLocation::Memory(a) => a, _ => return Err("LGDT requires memory operand".into()) };
                        let mut buf = [0u8; 10];
                        mem.read_bytes(addr, &mut buf).map_err(|e| e.to_string())?;
                        self.regs.gdtr.limit = u16::from_le_bytes([buf[0], buf[1]]);
                        self.regs.gdtr.base = u64::from_le_bytes([buf[2],buf[3],buf[4],buf[5],buf[6],buf[7],buf[8],buf[9]]);
                        Ok(None)
                    }
                    3 => { // LIDT
                        let addr = match rm_loc { OperandLocation::Memory(a) => a, _ => return Err("LIDT requires memory operand".into()) };
                        let mut buf = [0u8; 10];
                        mem.read_bytes(addr, &mut buf).map_err(|e| e.to_string())?;
                        self.regs.idtr.limit = u16::from_le_bytes([buf[0], buf[1]]);
                        self.regs.idtr.base = u64::from_le_bytes([buf[2],buf[3],buf[4],buf[5],buf[6],buf[7],buf[8],buf[9]]);
                        Ok(None)
                    }
                    0 => { // SGDT
                        if let OperandLocation::Memory(a) = rm_loc {
                            let mut buf = [0u8; 10];
                            buf[0..2].copy_from_slice(&self.regs.gdtr.limit.to_le_bytes());
                            buf[2..10].copy_from_slice(&self.regs.gdtr.base.to_le_bytes());
                            mem.write_bytes(a, &buf).map_err(|e| e.to_string())?;
                        }
                        Ok(None)
                    }
                    1 => { // SIDT
                        if let OperandLocation::Memory(a) = rm_loc {
                            let mut buf = [0u8; 10];
                            buf[0..2].copy_from_slice(&self.regs.idtr.limit.to_le_bytes());
                            buf[2..10].copy_from_slice(&self.regs.idtr.base.to_le_bytes());
                            mem.write_bytes(a, &buf).map_err(|e| e.to_string())?;
                        }
                        Ok(None)
                    }
                    _ => Ok(None), // other 0F 01 subops ignored
                }
            }
            // MOV r32/64, CRn (0F 20) / MOV CRn, r32/64 (0F 22) - minimal
            0x20 | 0x22 => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, rex);
                let cr = modrm.reg & 0x07;
                let rm = modrm.rm | if rex.b { 8 } else { 0 };
                if op2 == 0x20 {
                    let val = match cr { 0 => self.regs.cr0, 2 => self.regs.cr2, 3 => self.regs.cr3, 4 => self.regs.cr4, _ => 0 };
                    self.regs.set_reg64(rm, val);
                } else {
                    let val = self.regs.get_reg64(rm);
                    match cr { 0 => { self.regs.cr0 = val; self.regs.real_mode = (self.regs.cr0 & 1) == 0; }, 2 => self.regs.cr2 = val, 3 => self.regs.cr3 = val, 4 => self.regs.cr4 = val, _ => {} }
                }
                Ok(None)
            }
            // WRMSR/RDMSR minimal stubs
            0x30 => { // WRMSR
                let ecx = self.regs.ecx();
                let val = ((self.regs.edx() as u64) << 32) | (self.regs.eax() as u64);
                match ecx {
                    0x0000_001B => { /* IA32_APIC_BASE: store conceptually; device model not fully wired */ }
                    0x0000_0174 | 0x0000_0175 | 0x0000_0176 => { /* SYSENTER */ }
                    0xC000_0080 => { // EFER
                        self.regs.efer = val;
                        if self.regs.cr0_pg() && self.regs.efer_lme() { self.regs.efer |= 1 << 10; } else { self.regs.efer &= !(1 << 10); }
                    }
                    0xC000_0081..=0xC000_0084 => { /* STAR/LSTAR/CSTAR/SFMASK */ }
                    0xC000_0100..=0xC000_0102 => { /* FS/GS/KERNEL_GS base */ }
                    _ => {}
                }
                Ok(None)
            }
            0x32 => { // RDMSR
                let ecx = self.regs.ecx();
                let val = match ecx {
                    0x0000_001B => 0xFEE0_0900u64, // IA32_APIC_BASE stub
                    0xC000_0080 => self.regs.efer,
                    _ => 0,
                };
                self.regs.set_eax(val as u32);
                self.regs.set_edx((val >> 32) as u32);
                Ok(None)
            }
            // CPUID (0F A2) - minimal deterministic stub
            0xA2 => {
                let leaf = self.regs.eax();
                let sub = self.regs.ecx();
                let (a,b,c,d) = match leaf {
                    0x0 => (0x1, u32::from_le_bytes(*b"Caps"), u32::from_le_bytes(*b"uleO"), u32::from_le_bytes(*b"SVM ")),
                    0x1 => { let mut d = 0u32; d |= 1<<0; d |= 1<<4; d |= 1<<8; d |= 1<<15; d |= 1<<23; (0x0000_0600, 0, 0, d) },
                    0x8000_0000 => (0x8000_0001u32, 0, 0, 0),
                    0x8000_0001 => { let mut d = 0u32; d |= 1<<29; d |= 1<<20; let mut c = 0u32; c |= 1<<0; (0,0,c,d) },
                    _ => { let _ = sub; (0,0,0,0) }
                };
                self.regs.set_eax(a as u32);
                self.regs.set_ebx(b);
                self.regs.set_ecx(c);
                self.regs.set_edx(d);
                Ok(None)
            }
            // RDTSC (0F 31)
            0x31 => {
                let tsc = 0u64; // deterministic stub; real TSC/timers pending
                self.regs.set_eax(tsc as u32);
                self.regs.set_edx((tsc >> 32) as u32);
                Ok(None)
            }
            // SHRD/SHLD imm8 (0F AC/AD)
            0xAC | 0xAD => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                let count = (self.fetch_u8(mem)? & 0x3F) as u32;
                let reg_src = if is_64bit { self.regs.get_reg64(modrm.reg) } else if operand_size_16 { self.regs.get_reg16(modrm.reg) as u64 } else { self.regs.get_reg32(modrm.reg) as u64 };
                let dst = if operand_size_16 { rm_loc.read_u16(&self.regs, mem)? as u64 } else { rm_loc.read_val(is_64bit, &self.regs, mem)? };
                let bits = if is_64bit { 64u32 } else if operand_size_16 { 16u32 } else { 32u32 };
                let mask = if bits==64 { !0u64 } else { (1u64<<bits)-1 };
                let res = if count==0 || count>=bits { dst } else if op2==0xAC { (dst>>count) | ((reg_src<<(bits-count))&mask) } else { ((dst<<count) | (reg_src>>(bits-count))) & mask };
                let res = res & mask;
                if operand_size_16 { rm_loc.write_u16(res as u16, &mut self.regs, mem)?; } else { rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?; }
                Ok(None)
            },
            // Jcc rel32 (0F 80..8F)
            0x80..=0x8F => {
                let rel32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i32;
                self.regs.rip += 4;
                let cond = match op2 {
                    0x80 => self.regs.of(),
                    0x81 => !self.regs.of(),
                    0x82 => self.regs.cf(),
                    0x83 => !self.regs.cf(),
                    0x84 => self.regs.zf(),
                    0x85 => !self.regs.zf(),
                    0x86 => self.regs.cf() || self.regs.zf(),
                    0x87 => !self.regs.cf() && !self.regs.zf(),
                    0x88 => self.regs.sf(),
                    0x89 => !self.regs.sf(),
                    0x8A => self.regs.pf(),
                    0x8B => !self.regs.pf(),
                    0x8C => self.regs.sf() != self.regs.of(),
                    0x8D => self.regs.sf() == self.regs.of(),
                    0x8E => self.regs.zf() || (self.regs.sf() != self.regs.of()),
                    0x8F => !self.regs.zf() && (self.regs.sf() == self.regs.of()),
                    _ => false,
                };
                if cond { self.regs.rip = (self.regs.rip as i64).wrapping_add(rel32 as i64) as u64; if real_mode { self.regs.rip &= 0xFFFF; } }
                Ok(None)
            }
            // MOVZX r, r/m8 (0F B6) / r, r/m16 (0F B7)
            0xB6 | 0xB7 => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                let val = if op2 == 0xB6 { rm_loc.read_u8(rex, &self.regs, mem)? as u64 } else { rm_loc.read_u16(&self.regs, mem)? as u64 };
                if is_64bit { self.regs.set_reg64(modrm.reg, val); } else if operand_size_16 { self.regs.set_reg16(modrm.reg, val as u16); } else { self.regs.set_reg32(modrm.reg, val as u32); }
                Ok(None)
            }
            // MOVSX r, r/m8 (0F BE) / r, r/m16 (0F BF)
            0xBE | 0xBF => {
                let modrm_byte = self.fetch_u8(mem)?;
                let modrm = ModRm::parse(modrm_byte, rex);
                let seg = self.seg_override_base.unwrap_or(if real_mode { (self.regs.ds as u64)<<4 } else { 0 });
                let rm_loc = OperandLocation::resolve_rm_with_seg(&modrm, rex, &mut self.regs, mem, real_mode || addr_size_16, seg)?;
                let val = if op2 == 0xBE { (rm_loc.read_u8(rex, &self.regs, mem)? as i8 as i64) as u64 } else { (rm_loc.read_u16(&self.regs, mem)? as i16 as i64) as u64 };
                if is_64bit { self.regs.set_reg64(modrm.reg, val); } else if operand_size_16 { self.regs.set_reg16(modrm.reg, val as u16); } else { self.regs.set_reg32(modrm.reg, val as u32); }
                Ok(None)
            }
            _ => Ok(Some(CpuExitReason::InvalidOpcode(0x0F))),
        }
    }

    pub fn raise_software_interrupt(&mut self, vector: u8, mem: &mut dyn MemoryBus) -> Result<(), String> {
        self.deliver_interrupt(vector, mem, true)
    }

    pub fn deliver_external_interrupt(&mut self, vector: u8, mem: &mut dyn MemoryBus) -> Result<bool, String> {
        if !self.regs.interrupts_enabled { return Ok(false); }
        self.deliver_interrupt(vector, mem, false)?;
        Ok(true)
    }

    fn deliver_interrupt(&mut self, vector: u8, mem: &mut dyn MemoryBus, _software: bool) -> Result<(), String> {
        self.deliver_exception(vector, None, mem)
    }

    pub fn deliver_exception(&mut self, vector: u8, error_code: Option<u32>, mem: &mut dyn MemoryBus) -> Result<(), String> {
        let base = self.regs.idtr.base;
        let limit = self.regs.idtr.limit as u64;
        let off = (vector as u64) * 16;
        if base == 0 || off + 16 > limit.saturating_add(1) {
            return Err(format!("IDT not configured for vector 0x{vector:02X} err={error_code:?}"));
        }
        let entry_addr = GuestAddress::new(base + off);
        let mut entry = [0u8; 16];
        mem.read_bytes(entry_addr, &mut entry).map_err(|e| e.to_string())?;
        let off_lo = u16::from_le_bytes([entry[0], entry[1]]) as u64;
        let selector = u16::from_le_bytes([entry[2], entry[3]]) as u64;
        let _ist = entry[4] & 0x7;
        let attr = entry[5];
        let present = (attr & 0x80) != 0;
        let gate_type = attr & 0x0F;
        let dpl = (attr >> 5) & 0x3;
        let _ = (gate_type, dpl); // minimal: no full privilege/gate checks yet
        let off_mid = u16::from_le_bytes([entry[6], entry[7]]) as u64;
        let off_hi = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as u64;
        let handler = off_lo | (off_mid << 16) | (off_hi << 32);
        if !present || handler == 0 {
            return Err(format!("IDT entry not present for vector 0x{vector:02X}"));
        }
        let rflags = self.regs.rflags;
        self.push_u64(mem, rflags)?;
        self.push_u64(mem, selector as u64)?;
        self.push_u64(mem, self.regs.rip)?;
        if let Some(err) = error_code {
            self.push_u64(mem, err as u64)?;
        }
        self.regs.interrupts_enabled = false;
        self.regs.rflags &= !FLAG_IF;
        self.regs.rip = handler;
        Ok(())
    }
}
