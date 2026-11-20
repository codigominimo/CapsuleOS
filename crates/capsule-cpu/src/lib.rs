//! # capsule-cpu
//!
//! Modelo de ejecución de la CPU x86-64 y registros arquitecturales.

pub mod registers;

pub use registers::Registers;

use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;
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

/// Procesador virtual emulado básico para el hito M1.
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

    /// Ejecuta una sola instrucción en memoria física a partir de `regs.rip`.
    pub fn step(&mut self, mem: &mut GuestMemory) -> Result<Option<CpuExitReason>, String> {
        let pc = GuestAddress::new(self.regs.rip);
        let opcode = mem
            .read_u8(pc)
            .map_err(|e| format!("Error leyendo opcode en {}: {}", pc, e))?;

        self.regs.rip += 1;

        match opcode {
            // NOP
            0x90 => Ok(None),

            // HLT
            0xF4 => Ok(Some(CpuExitReason::Halt)),

            // MOV r32, imm32 (0xB8 + reg)
            0xB8..=0xBF => {
                let reg_idx = opcode - 0xB8;
                let imm_addr = GuestAddress::new(self.regs.rip);
                let imm = mem
                    .read_u32(imm_addr)
                    .map_err(|e| format!("Error leyendo imm32 en {}: {}", imm_addr, e))?;
                self.regs.rip += 4;

                match reg_idx {
                    0 => self.regs.set_eax(imm),
                    1 => self.regs.set_ecx(imm),
                    2 => self.regs.set_edx(imm),
                    3 => self.regs.set_ebx(imm),
                    4 => self.regs.rsp = imm as u64,
                    5 => self.regs.rbp = imm as u64,
                    6 => self.regs.rsi = imm as u64,
                    7 => self.regs.rdi = imm as u64,
                    _ => unreachable!(),
                }
                Ok(None)
            }

            // ADD r/m32, imm8 (0x83)
            0x83 => {
                let modrm = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo ModRM: {}", e))?;
                self.regs.rip += 1;

                let imm = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo imm8: {}", e))? as i8 as i32;
                self.regs.rip += 1;

                // ModRM 0xC0 = ADD EAX, imm8
                if modrm == 0xC0 {
                    let cur = self.regs.eax() as i32;
                    self.regs.set_eax((cur.wrapping_add(imm)) as u32);
                    Ok(None)
                } else {
                    Err(format!("ModRM 0x{:02X} no implementado para 0x83", modrm))
                }
            }

            // INC r/m32 (0xFF /0)
            0xFF => {
                let modrm = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo ModRM: {}", e))?;
                self.regs.rip += 1;

                // ModRM 0xC0 = INC EAX
                if modrm == 0xC0 {
                    let cur = self.regs.eax();
                    self.regs.set_eax(cur.wrapping_add(1));
                    Ok(None)
                } else {
                    Err(format!("ModRM 0x{:02X} no implementado para 0xFF", modrm))
                }
            }

            // OUT imm8, AL (0xE6)
            0xE6 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo puerto E/S: {}", e))? as u16;
                self.regs.rip += 1;
                let data = (self.regs.rax & 0xFF) as u32;
                Ok(Some(CpuExitReason::IoOut {
                    port,
                    data,
                    size: 1,
                }))
            }

            // OUT imm8, EAX (0xE7)
            0xE7 => {
                let port = mem
                    .read_u8(GuestAddress::new(self.regs.rip))
                    .map_err(|e| format!("Error leyendo puerto E/S: {}", e))? as u16;
                self.regs.rip += 1;
                let data = self.regs.eax();
                Ok(Some(CpuExitReason::IoOut {
                    port,
                    data,
                    size: 4,
                }))
            }

            other => Ok(Some(CpuExitReason::InvalidOpcode(other))),
        }
    }
}
