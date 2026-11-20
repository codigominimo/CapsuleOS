//! Decodificación de ModR/M, prefijo REX y modos de direccionamiento de x86-64.

use crate::registers::Registers;
use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;

#[derive(Debug, Clone, Copy, Default)]
pub struct RexPrefix {
    pub present: bool,
    pub w: bool, // Operando de 64 bits si true
    pub r: bool, // Extensión del campo 'reg' (+8)
    pub x: bool, // Extensión del campo 'index' de SIB (+8)
    pub b: bool, // Extensión del campo 'r/m' o 'base' (+8)
}

impl RexPrefix {
    pub fn parse(byte: u8) -> Option<Self> {
        if (byte & 0xF0) == 0x40 {
            Some(Self {
                present: true,
                w: (byte & 0x08) != 0,
                r: (byte & 0x04) != 0,
                x: (byte & 0x02) != 0,
                b: (byte & 0x01) != 0,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModRm {
    pub mode: u8,
    pub reg: u8,
    pub rm: u8,
}

impl ModRm {
    pub fn parse(byte: u8, rex: &RexPrefix) -> Self {
        let mode = (byte >> 6) & 0x03;
        let reg_raw = (byte >> 3) & 0x07;
        let rm_raw = byte & 0x07;

        let reg = if rex.r { reg_raw + 8 } else { reg_raw };
        let rm = if rex.b { rm_raw + 8 } else { rm_raw };

        Self { mode, reg, rm }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OperandLocation {
    Register(u8),
    Memory(GuestAddress),
}

impl OperandLocation {
    pub fn resolve_rm(
        modrm: &ModRm,
        rex: &RexPrefix,
        regs: &mut Registers,
        mem: &GuestMemory,
    ) -> Result<Self, String> {
        if modrm.mode == 3 {
            Ok(OperandLocation::Register(modrm.rm))
        } else {
            let mut addr: u64;

            if (modrm.rm & 0x07) == 5 && modrm.mode == 0 {
                let disp = mem
                    .read_u32(GuestAddress::new(regs.rip))
                    .map_err(|e| format!("Error leyendo disp32 para RIP-relative: {}", e))? as i32 as i64;
                regs.rip += 4;
                addr = (regs.rip as i64).wrapping_add(disp) as u64;
            } else if (modrm.rm & 0x07) == 4 {
                let sib = mem
                    .read_u8(GuestAddress::new(regs.rip))
                    .map_err(|e| format!("Error leyendo byte SIB: {}", e))?;
                regs.rip += 1;

                let scale = 1usize << ((sib >> 6) & 0x03);
                let index_raw = (sib >> 3) & 0x07;
                let base_raw = sib & 0x07;

                let index_reg = if rex.x { index_raw + 8 } else { index_raw };
                let base_reg = if rex.b { base_raw + 8 } else { base_raw };

                let base_val = regs.get_reg64(base_reg);
                let index_val = if index_raw == 4 && !rex.x {
                    0
                } else {
                    regs.get_reg64(index_reg)
                };

                addr = base_val.wrapping_add(index_val.wrapping_mul(scale as u64));

                if modrm.mode == 1 {
                    let disp = mem
                        .read_u8(GuestAddress::new(regs.rip))
                        .map_err(|e| format!("Error leyendo disp8 en SIB: {}", e))? as i8 as i64;
                    regs.rip += 1;
                    addr = (addr as i64).wrapping_add(disp) as u64;
                } else if modrm.mode == 2 {
                    let disp = mem
                        .read_u32(GuestAddress::new(regs.rip))
                        .map_err(|e| format!("Error leyendo disp32 en SIB: {}", e))? as i32 as i64;
                    regs.rip += 4;
                    addr = (addr as i64).wrapping_add(disp) as u64;
                }
            } else {
                let base_val = regs.get_reg64(modrm.rm);
                addr = base_val;

                if modrm.mode == 1 {
                    let disp = mem
                        .read_u8(GuestAddress::new(regs.rip))
                        .map_err(|e| format!("Error leyendo disp8: {}", e))? as i8 as i64;
                    regs.rip += 1;
                    addr = (addr as i64).wrapping_add(disp) as u64;
                } else if modrm.mode == 2 {
                    let disp = mem
                        .read_u32(GuestAddress::new(regs.rip))
                        .map_err(|e| format!("Error leyendo disp32: {}", e))? as i32 as i64;
                    regs.rip += 4;
                    addr = (addr as i64).wrapping_add(disp) as u64;
                }
            }

            Ok(OperandLocation::Memory(GuestAddress::new(addr)))
        }
    }

    /// Lee un byte del operando (modo 8 bits).
    pub fn read_u8(&self, rex: &RexPrefix, regs: &Registers, mem: &GuestMemory) -> Result<u8, String> {
        match *self {
            OperandLocation::Register(reg) => Ok(regs.get_reg8(reg, rex.present)),
            OperandLocation::Memory(addr) => {
                mem.read_u8(addr).map_err(|e| format!("Fallo leyendo u8 en {}: {}", addr, e))
            }
        }
    }

    /// Escribe un byte en el operando (modo 8 bits).
    pub fn write_u8(&self, val: u8, rex: &RexPrefix, regs: &mut Registers, mem: &mut GuestMemory) -> Result<(), String> {
        match *self {
            OperandLocation::Register(reg) => {
                regs.set_reg8(reg, val, rex.present);
                Ok(())
            }
            OperandLocation::Memory(addr) => {
                mem.write_u8(addr, val).map_err(|e| format!("Fallo escribiendo u8 en {}: {}", addr, e))
            }
        }
    }

    pub fn read_val(
        &self,
        is_64bit: bool,
        regs: &Registers,
        mem: &GuestMemory,
    ) -> Result<u64, String> {
        match *self {
            OperandLocation::Register(reg) => {
                if is_64bit {
                    Ok(regs.get_reg64(reg))
                } else {
                    Ok(regs.get_reg32(reg) as u64)
                }
            }
            OperandLocation::Memory(addr) => {
                if is_64bit {
                    mem.read_u64(addr)
                        .map_err(|e| format!("Fallo leyendo u64 en {}: {}", addr, e))
                } else {
                    mem.read_u32(addr)
                        .map(|v| v as u64)
                        .map_err(|e| format!("Fallo leyendo u32 en {}: {}", addr, e))
                }
            }
        }
    }

    pub fn write_val(
        &self,
        val: u64,
        is_64bit: bool,
        regs: &mut Registers,
        mem: &mut GuestMemory,
    ) -> Result<(), String> {
        match *self {
            OperandLocation::Register(reg) => {
                if is_64bit {
                    regs.set_reg64(reg, val);
                } else {
                    regs.set_reg32(reg, val as u32);
                }
                Ok(())
            }
            OperandLocation::Memory(addr) => {
                if is_64bit {
                    mem.write_bytes(addr, &val.to_le_bytes())
                        .map_err(|e| format!("Fallo escribiendo u64 en {}: {}", addr, e))
                } else {
                    let val32 = val as u32;
                    mem.write_bytes(addr, &val32.to_le_bytes())
                        .map_err(|e| format!("Fallo escribiendo u32 en {}: {}", addr, e))
                }
            }
        }
    }
}
