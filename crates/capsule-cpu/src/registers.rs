//! Archivo de registros de la arquitectura x86-64.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            rflags: 0x0002, // Bit 1 siempre debe estar activo en x86
            ..Default::default()
        }
    }

    /// Retorna los 32 bits inferiores de RAX (EAX).
    #[inline]
    pub fn eax(&self) -> u32 {
        self.rax as u32
    }

    /// Establece EAX con extensión a cero de los 32 bits superiores en modo 64 bits.
    #[inline]
    pub fn set_eax(&mut self, val: u32) {
        self.rax = val as u64;
    }

    /// Retorna los 32 bits inferiores de RBX (EBX).
    #[inline]
    pub fn ebx(&self) -> u32 {
        self.rbx as u32
    }

    #[inline]
    pub fn set_ebx(&mut self, val: u32) {
        self.rbx = val as u64;
    }

    /// Retorna los 32 bits inferiores de RCX (ECX).
    #[inline]
    pub fn ecx(&self) -> u32 {
        self.rcx as u32
    }

    #[inline]
    pub fn set_ecx(&mut self, val: u32) {
        self.rcx = val as u64;
    }

    /// Retorna los 32 bits inferiores de RDX (EDX).
    #[inline]
    pub fn edx(&self) -> u32 {
        self.rdx as u32
    }

    #[inline]
    pub fn set_edx(&mut self, val: u32) {
        self.rdx = val as u64;
    }
}

impl fmt::Display for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "RAX: 0x{:016X}  RBX: 0x{:016X}", self.rax, self.rbx)?;
        writeln!(f, "RCX: 0x{:016X}  RDX: 0x{:016X}", self.rcx, self.rdx)?;
        writeln!(f, "RSI: 0x{:016X}  RDI: 0x{:016X}", self.rsi, self.rdi)?;
        writeln!(f, "RBP: 0x{:016X}  RSP: 0x{:016X}", self.rbp, self.rsp)?;
        writeln!(f, "R8 : 0x{:016X}  R9 : 0x{:016X}", self.r8, self.r9)?;
        writeln!(f, "R10: 0x{:016X}  R11: 0x{:016X}", self.r10, self.r11)?;
        writeln!(f, "R12: 0x{:016X}  R13: 0x{:016X}", self.r12, self.r13)?;
        writeln!(f, "R14: 0x{:016X}  R15: 0x{:016X}", self.r14, self.r15)?;
        writeln!(f, "RIP: 0x{:016X}  RFLAGS: 0x{:016X}", self.rip, self.rflags)
    }
}
