//! Archivo de registros de la arquitectura x86-64 y manipulación de RFLAGS.

use crate::flags::*;
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
            rflags: FLAG_RESERVED,
            ..Default::default()
        }
    }

    /// Retorna el valor de un registro de 64 bits por su índice x86-64 (0 a 15).
    pub fn get_reg64(&self, idx: u8) -> u64 {
        match idx & 0x0F {
            0 => self.rax,
            1 => self.rcx,
            2 => self.rdx,
            3 => self.rbx,
            4 => self.rsp,
            5 => self.rbp,
            6 => self.rsi,
            7 => self.rdi,
            8 => self.r8,
            9 => self.r9,
            10 => self.r10,
            11 => self.r11,
            12 => self.r12,
            13 => self.r13,
            14 => self.r14,
            15 => self.r15,
            _ => unreachable!(),
        }
    }

    /// Establece el valor de un registro de 64 bits por su índice (0 a 15).
    pub fn set_reg64(&mut self, idx: u8, val: u64) {
        match idx & 0x0F {
            0 => self.rax = val,
            1 => self.rcx = val,
            2 => self.rdx = val,
            3 => self.rbx = val,
            4 => self.rsp = val,
            5 => self.rbp = val,
            6 => self.rsi = val,
            7 => self.rdi = val,
            8 => self.r8 = val,
            9 => self.r9 = val,
            10 => self.r10 = val,
            11 => self.r11 = val,
            12 => self.r12 = val,
            13 => self.r13 = val,
            14 => self.r14 = val,
            15 => self.r15 = val,
            _ => unreachable!(),
        }
    }

    /// Retorna el valor de 32 bits de un registro.
    pub fn get_reg32(&self, idx: u8) -> u32 {
        self.get_reg64(idx) as u32
    }

    /// Establece el valor de 32 bits (con extensión a cero en los 32 bits superiores como dicta x86-64).
    pub fn set_reg32(&mut self, idx: u8, val: u32) {
        self.set_reg64(idx, val as u64);
    }

    #[inline]
    pub fn eax(&self) -> u32 {
        self.rax as u32
    }

    #[inline]
    pub fn set_eax(&mut self, val: u32) {
        self.rax = val as u64;
    }

    #[inline]
    pub fn ebx(&self) -> u32 {
        self.rbx as u32
    }

    #[inline]
    pub fn set_ebx(&mut self, val: u32) {
        self.rbx = val as u64;
    }

    #[inline]
    pub fn ecx(&self) -> u32 {
        self.rcx as u32
    }

    #[inline]
    pub fn set_ecx(&mut self, val: u32) {
        self.rcx = val as u64;
    }

    #[inline]
    pub fn edx(&self) -> u32 {
        self.rdx as u32
    }

    #[inline]
    pub fn set_edx(&mut self, val: u32) {
        self.rdx = val as u64;
    }

    // Consultas de RFLAGS
    #[inline]
    pub fn zf(&self) -> bool {
        (self.rflags & FLAG_ZF) != 0
    }

    #[inline]
    pub fn cf(&self) -> bool {
        (self.rflags & FLAG_CF) != 0
    }

    #[inline]
    pub fn sf(&self) -> bool {
        (self.rflags & FLAG_SF) != 0
    }

    #[inline]
    pub fn of(&self) -> bool {
        (self.rflags & FLAG_OF) != 0
    }

    #[inline]
    pub fn pf(&self) -> bool {
        (self.rflags & FLAG_PF) != 0
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
        writeln!(
            f,
            "RIP: 0x{:016X}  RFLAGS: 0x{:016X} [CF:{} ZF:{} SF:{} OF:{}]",
            self.rip,
            self.rflags,
            self.cf() as u8,
            self.zf() as u8,
            self.sf() as u8,
            self.of() as u8
        )
    }
}
