//! Minimal Local APIC + IOAPIC stubs for MMIO dispatch and IRQ routing scaffolding.
//! This is NOT full architectural APIC emulation; it provides address decode, register
//! storage, and deterministic interrupt routing hooks for incremental unblock work.

#[derive(Debug, Clone)]
pub struct LocalApic {
    base: u64,
    regs: [u32; 0x40], // 256 bytes window, indexed by reg offset>>4
    id: u32,
    svr: u32,
    icr_low: u32,
    icr_high: u32,
    lvt_timer: u32,
    lvt_lint0: u32,
    lvt_lint1: u32,
    lvt_error: u32,
    eoi_pending: bool,
}

impl Default for LocalApic {
    fn default() -> Self { Self::new(0xFEE0_0000) }
}

impl LocalApic {
    pub fn new(base: u64) -> Self {
        let mut s = Self {
            base,
            regs: [0; 0x40],
            id: 0,
            svr: 0x0001_00FF, // masked reset-ish
            icr_low: 0,
            icr_high: 0,
            lvt_timer: 0x1_0000, // masked
            lvt_lint0: 0x1_0000,
            lvt_lint1: 0x1_0000,
            lvt_error: 0x1_0000,
            eoi_pending: false,
        };
        s.regs[0x2] = s.id;
        s.regs[0xF] = s.svr;
        s
    }

    pub fn base(&self) -> u64 { self.base }
    pub fn set_base(&mut self, base: u64) { self.base = base & 0xFFFF_F000; }
    pub fn handles_mmio(&self, addr: u64) -> bool { addr >= self.base && addr < self.base + 0x1000 }

    pub fn read_mmio(&self, addr: u64, size: u8) -> u64 {
        let off = ((addr - self.base) & 0xFF0) as usize >> 4;
        let val = match off {
            0x2 => self.id,
            0xB => 0, // EOI read undefined; return 0
            0xF => self.svr,
            0x30 => self.icr_low,
            0x31 => self.icr_high,
            0x32 => self.lvt_timer,
            0x35 => self.lvt_lint0,
            0x36 => self.lvt_lint1,
            0x37 => self.lvt_error,
            _ => *self.regs.get(off).unwrap_or(&0),
        };
        match size { 1 => (val & 0xFF) as u64, 2 => (val & 0xFFFF) as u64, _ => val as u64 }
    }

    pub fn write_mmio(&mut self, addr: u64, data: u64, size: u8) {
        let off = ((addr - self.base) & 0xFF0) as usize >> 4;
        let val = match size { 1 => data as u32 & 0xFF, 2 => data as u32 & 0xFFFF, _ => data as u32 };
        if let Some(slot) = self.regs.get_mut(off) { *slot = val; }
        match off {
            0x2 => self.id = val,
            0xB => { self.eoi_pending = false; } // EOI
            0xF => self.svr = val,
            0x30 => self.icr_low = val,
            0x31 => self.icr_high = val,
            0x32 => self.lvt_timer = val,
            0x35 => self.lvt_lint0 = val,
            0x36 => self.lvt_lint1 = val,
            0x37 => self.lvt_error = val,
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoApic {
    base: u64,
    ioregsel: u32,
    id: u32,
    ver: u32,
    arb: u32,
    redtbl: [u32; 64], // 24 entries * 2 dwords typical, reserve 64 dwords
}

impl Default for IoApic {
    fn default() -> Self { Self::new(0xFEC0_0000) }
}

impl IoApic {
    pub fn new(base: u64) -> Self {
        Self {
            base,
            ioregsel: 0,
            id: 0,
            ver: 0x11 | (23 << 16), // version 11h, 24 redir entries-1
            arb: 0,
            redtbl: [0x0001_0000; 64], // masked
        }
    }

    pub fn base(&self) -> u64 { self.base }
    pub fn handles_mmio(&self, addr: u64) -> bool { addr >= self.base && addr < self.base + 0x20 }

    pub fn read_mmio(&self, addr: u64, size: u8) -> u64 {
        let off = (addr - self.base) & 0x1F;
        let val = match off {
            0x00 => self.ioregsel,
            0x10 => self.read_reg(self.ioregsel),
            _ => 0,
        };
        match size { 1 => (val & 0xFF) as u64, 2 => (val & 0xFFFF) as u64, _ => val as u64 }
    }

    pub fn write_mmio(&mut self, addr: u64, data: u64, size: u8) {
        let off = (addr - self.base) & 0x1F;
        let val = match size { 1 => data as u32 & 0xFF, 2 => data as u32 & 0xFFFF, _ => data as u32 };
        match off {
            0x00 => self.ioregsel = val & 0xFF,
            0x10 => self.write_reg(self.ioregsel, val),
            _ => {}
        }
    }

    fn read_reg(&self, reg: u32) -> u32 {
        match reg {
            0x00 => self.id,
            0x01 => self.ver,
            0x02 => self.arb,
            0x10..=0x3F => self.redtbl.get((reg - 0x10) as usize).copied().unwrap_or(0),
            _ => 0,
        }
    }

    fn write_reg(&mut self, reg: u32, val: u32) {
        match reg {
            0x00 => self.id = val & 0x0F00_0000,
            0x01 | 0x02 => {}
            0x10..=0x3F => { if let Some(s) = self.redtbl.get_mut((reg - 0x10) as usize) { *s = val; } }
            _ => {}
        }
    }
}
