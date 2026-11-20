//! # capsule-cpu
//!
//! Modelo de ejecución de la CPU x86-64, registros arquitecturales y decodificador/ejecutor de instrucciones.

pub mod flags;
pub mod modrm;
pub mod registers;

pub use flags::*;
pub use modrm::{ModRm, RexPrefix};
pub use registers::Registers;

use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;
use modrm::OperandLocation;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum CpuExitReason {
    Halt,
    IoIn { port: u16, size: u8 },
    IoOut { port: u16, data: u32, size: u8 },
    InvalidOpcode(u8),
    MemoryFault(String),
}

impl fmt::Display for CpuExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpuExitReason::Halt => write!(f, "CPU ejecutó HLT"),
            CpuExitReason::IoIn { port, size } => {
                write!(f, "E/S Lectura Puerto 0x{:04X} ({} bytes)", port, size)
            }
            CpuExitReason::IoOut { port, data, size } => {
                write!(
                    f,
                    "E/S Escritura Puerto 0x{:04X} <- 0x{:X} ({} bytes)",
                    port, data, size
                )
            }
            CpuExitReason::InvalidOpcode(op) => write!(f, "Opcode no válido: 0x{:02X}", op),
            CpuExitReason::MemoryFault(msg) => write!(f, "Fallo de memoria: {}", msg),
        }
    }
}

/// Procesador virtual emulado x86-64.
pub struct SimpleVcpu {
    pub id: usize,
    pub regs: Registers,
}

impl SimpleVcpu {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            regs: Registers::new(),
        }
    }

    /// Empuja un valor de 64 bits en la pila ([RSP - 8] <- val; RSP <- RSP - 8).
    pub fn push_u64(&mut self, mem: &mut GuestMemory, val: u64) -> Result<(), String> {
        self.regs.rsp = self.regs.rsp.wrapping_sub(8);
        let sp = GuestAddress::new(self.regs.rsp);
        mem.write_bytes(sp, &val.to_le_bytes())
            .map_err(|e| format!("Fallo en PUSH u64 a {}: {}", sp, e))
    }

    /// Extrae un valor de 64 bits de la pila (val <- [RSP]; RSP <- RSP + 8).
    pub fn pop_u64(&mut self, mem: &GuestMemory) -> Result<u64, String> {
        let sp = GuestAddress::new(self.regs.rsp);
        let val = mem
            .read_u64(sp)
            .map_err(|e| format!("Fallo en POP u64 de {}: {}", sp, e))?;
        self.regs.rsp = self.regs.rsp.wrapping_add(8);
        Ok(val)
    }

    /// Ejecuta una sola instrucción en memoria física a partir de `regs.rip`.
    pub fn step(&mut self, mem: &mut GuestMemory) -> Result<Option<CpuExitReason>, String> {
        // 1. Decodificar prefijo REX opcional (0x40 - 0x4F)
        let mut rex = RexPrefix::default();
        let mut opcode = mem
            .read_u8(GuestAddress::new(self.regs.rip))
            .map_err(|e| format!("Error leyendo opcode en 0x{:016X}: {}", self.regs.rip, e))?;

        if let Some(parsed_rex) = RexPrefix::parse(opcode) {
            rex = parsed_rex;
            self.regs.rip += 1;
            opcode = mem
                .read_u8(GuestAddress::new(self.regs.rip))
                .map_err(|e| format!("Error leyendo opcode tras REX: {}", e))?;
        }

        self.regs.rip += 1;
        let is_64bit = rex.w;

        match opcode {
            // NOP
            0x90 => Ok(None),

            // HLT
            0xF4 => Ok(Some(CpuExitReason::Halt)),

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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                rm_loc.write_val(reg_val, is_64bit, &mut self.regs, mem)?;
                Ok(None)
            }

            // MOV r/m8, r8 (0x88)
            0x88 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = self.regs.get_reg8(modrm.reg, rex.present);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                rm_loc.write_u8(reg_val, &rex, &mut self.regs, mem)?;
                Ok(None)
            }

            // MOV r8, r/m8 (0x8A)
            0x8A => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let byte_val = rm_loc.read_u8(&rex, &self.regs, mem)?;
                self.regs.set_reg8(modrm.reg, byte_val, rex.present);
                Ok(None)
            }

            // TEST r/m8, r8 (0x84)
            0x84 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = self.regs.get_reg8(modrm.reg, rex.present);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let rm_val = rm_loc.read_u8(&rex, &self.regs, mem)?;
                let res = rm_val & reg_val;
                update_logic_flags_u64(&mut self.regs.rflags, res as u64, false);
                Ok(None)
            }

            // MOV r, r/m (0x8B)
            0x8B => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let val = rm_loc.read_val(is_64bit, &self.regs, mem)?;
                if is_64bit {
                    self.regs.set_reg64(modrm.reg, val);
                } else {
                    self.regs.set_reg32(modrm.reg, val as u32);
                }
                Ok(None)
            }

            // MOV r/m, imm32 (0xC7 /0)
            0xC7 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let imm32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 4;
                let val = if is_64bit { imm32 as i32 as i64 as u64 } else { imm32 as u64 };
                rm_loc.write_val(val, is_64bit, &mut self.regs, mem)?;
                Ok(None)
            }

            // ADD r/m, r (0x01)
            0x01 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let reg_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;

                if is_64bit {
                    let res = dst_val.wrapping_add(reg_val);
                    update_add_flags_u64(&mut self.regs.rflags, dst_val, reg_val, res);
                    rm_loc.write_val(res, true, &mut self.regs, mem)?;
                } else {
                    let res = (dst_val as u32).wrapping_add(reg_val as u32);
                    update_add_flags_u32(&mut self.regs.rflags, dst_val as u32, reg_val as u32, res);
                    rm_loc.write_val(res as u64, false, &mut self.regs, mem)?;
                }
                Ok(None)
            }

            // ADD r, r/m (0x03)
            0x03 => {
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let src_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;
                let dst_val = if is_64bit { self.regs.get_reg64(modrm.reg) } else { self.regs.get_reg32(modrm.reg) as u64 };

                if is_64bit {
                    let res = dst_val.wrapping_add(src_val);
                    update_add_flags_u64(&mut self.regs.rflags, dst_val, src_val, res);
                    self.regs.set_reg64(modrm.reg, res);
                } else {
                    let res = (dst_val as u32).wrapping_add(src_val as u32);
                    update_add_flags_u32(&mut self.regs.rflags, dst_val as u32, src_val as u32, res);
                    self.regs.set_reg32(modrm.reg, res);
                }
                Ok(None)
            }

            // Grupo 1: ADD, OR, AND, SUB, XOR, CMP con imm32 (0x81) o imm8 (0x83)
            0x81 | 0x83 => {
                let is_imm8 = opcode == 0x83;
                let modrm_byte = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                self.regs.rip += 1;
                let modrm = ModRm::parse(modrm_byte, &rex);
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
                let dst_val = rm_loc.read_val(is_64bit, &self.regs, mem)?;

                let imm: u64 = if is_imm8 {
                    let imm8 = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i8;
                    self.regs.rip += 1;
                    if is_64bit { imm8 as i64 as u64 } else { imm8 as i32 as u32 as u64 }
                } else {
                    let imm32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())?;
                    self.regs.rip += 4;
                    if is_64bit { imm32 as i32 as i64 as u64 } else { imm32 as u64 }
                };

                match modrm.reg & 0x07 {
                    0 => {
                        // ADD
                        if is_64bit {
                            let res = dst_val.wrapping_add(imm);
                            update_add_flags_u64(&mut self.regs.rflags, dst_val, imm, res);
                            rm_loc.write_val(res, true, &mut self.regs, mem)?;
                        } else {
                            let res = (dst_val as u32).wrapping_add(imm as u32);
                            update_add_flags_u32(&mut self.regs.rflags, dst_val as u32, imm as u32, res);
                            rm_loc.write_val(res as u64, false, &mut self.regs, mem)?;
                        }
                    }
                    1 => {
                        // OR
                        let res = dst_val | imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res, is_64bit);
                        rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?;
                    }
                    4 => {
                        // AND
                        let res = dst_val & imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res, is_64bit);
                        rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?;
                    }
                    5 => {
                        // SUB
                        if is_64bit {
                            let res = dst_val.wrapping_sub(imm);
                            update_sub_flags_u64(&mut self.regs.rflags, dst_val, imm, res);
                            rm_loc.write_val(res, true, &mut self.regs, mem)?;
                        } else {
                            let res = (dst_val as u32).wrapping_sub(imm as u32);
                            update_sub_flags_u32(&mut self.regs.rflags, dst_val as u32, imm as u32, res);
                            rm_loc.write_val(res as u64, false, &mut self.regs, mem)?;
                        }
                    }
                    6 => {
                        // XOR
                        let res = dst_val ^ imm;
                        update_logic_flags_u64(&mut self.regs.rflags, res, is_64bit);
                        rm_loc.write_val(res, is_64bit, &mut self.regs, mem)?;
                    }
                    7 => {
                        // CMP (No escribe el resultado en destino, solo actualiza RFLAGS)
                        if is_64bit {
                            let res = dst_val.wrapping_sub(imm);
                            update_sub_flags_u64(&mut self.regs.rflags, dst_val, imm, res);
                        } else {
                            let res = (dst_val as u32).wrapping_sub(imm as u32);
                            update_sub_flags_u32(&mut self.regs.rflags, dst_val as u32, imm as u32, res);
                        }
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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
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
                let rm_loc = OperandLocation::resolve_rm(&modrm, &rex, &mut self.regs, mem)?;
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
                    other => return Err(format!("Operación 0xFF /{} no implementada", other)),
                }
                Ok(None)
            }

            // PUSH r64 (0x50..=0x57)
            0x50..=0x57 => {
                let reg_idx = (opcode - 0x50) + if rex.b { 8 } else { 0 };
                let val = self.regs.get_reg64(reg_idx);
                self.push_u64(mem, val)?;
                Ok(None)
            }

            // POP r64 (0x58..=0x5F)
            0x58..=0x5F => {
                let reg_idx = (opcode - 0x58) + if rex.b { 8 } else { 0 };
                let val = self.pop_u64(mem)?;
                self.regs.set_reg64(reg_idx, val);
                Ok(None)
            }

            // JMP rel8 (0xEB)
            0xEB => {
                let rel8 = mem.read_u8(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i8;
                self.regs.rip += 1;
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel8 as i64) as u64;
                Ok(None)
            }

            // JMP rel32 (0xE9)
            0xE9 => {
                let rel32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i32;
                self.regs.rip += 4;
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel32 as i64) as u64;
                Ok(None)
            }

            // CALL rel32 (0xE8)
            0xE8 => {
                let rel32 = mem.read_u32(GuestAddress::new(self.regs.rip)).map_err(|e| e.to_string())? as i32;
                self.regs.rip += 4;
                // Empujar la dirección de retorno en la pila
                let ret_addr = self.regs.rip;
                self.push_u64(mem, ret_addr)?;
                // Saltar al destino
                self.regs.rip = (self.regs.rip as i64).wrapping_add(rel32 as i64) as u64;
                Ok(None)
            }

            // RET (0xC3)
            0xC3 => {
                let ret_addr = self.pop_u64(mem)?;
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

            // IN EAX, imm8 (0xE5)
            0xE5 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                Ok(Some(CpuExitReason::IoIn { port, size: 4 }))
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

            // OUT imm8, EAX (0xE7)
            0xE7 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| e.to_string())? as u16;
                self.regs.rip += 1;
                let data = self.regs.eax();
                Ok(Some(CpuExitReason::IoOut { port, data, size: 4 }))
            }

            // IN AL, DX (0xEC)
            0xEC => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                Ok(Some(CpuExitReason::IoIn { port, size: 1 }))
            }

            // IN EAX, DX (0xED)
            0xED => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                Ok(Some(CpuExitReason::IoIn { port, size: 4 }))
            }

            // OUT DX, AL (0xEE)
            0xEE => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                let data = (self.regs.rax & 0xFF) as u32;
                Ok(Some(CpuExitReason::IoOut { port, data, size: 1 }))
            }

            // OUT DX, EAX (0xEF)
            0xEF => {
                let port = (self.regs.rdx & 0xFFFF) as u16;
                let data = self.regs.eax();
                Ok(Some(CpuExitReason::IoOut { port, data, size: 4 }))
            }

            other => Ok(Some(CpuExitReason::InvalidOpcode(other))),
        }
    }
}
