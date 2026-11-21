//! Minimal HPET MMIO stub.

#[derive(Debug, Clone)]
pub struct Hpet {
    base: u64,
    gen_cap: u64,
    gen_cfg: u64,
    main_counter: u64,
}

impl Default for Hpet {
    fn default() -> Self { Self::new(0xFED0_0000) }
}

impl Hpet {
    pub fn new(base: u64) -> Self {
        Self {
            base,
            gen_cap: 0x8086_A201_0000_0000, // vendor/cap stub
            gen_cfg: 0,
            main_counter: 0,
        }
    }

    pub fn base(&self) -> u64 { self.base }
    pub fn handles_mmio(&self, addr: u64) -> bool { addr >= self.base && addr < self.base + 0x400 }
    pub fn tick(&mut self, cycles: u64) {
        if self.gen_cfg & 1 != 0 { self.main_counter = self.main_counter.wrapping_add(cycles); }
    }

    pub fn read_mmio(&self, addr: u64, size: u8) -> u64 {
        let off = (addr - self.base) & 0x3FF;
        let val = match off {
            0x00 => self.gen_cap,
            0x10 => self.gen_cfg,
            0xF0 => self.main_counter,
            _ => 0,
        };
        mask_size(val, size)
    }

    pub fn write_mmio(&mut self, addr: u64, data: u64, size: u8) {
        let off = (addr - self.base) & 0x3FF;
        let val = merge_size(self.read_mmio(addr, 8), data, size);
        match off {
            0x10 => self.gen_cfg = val & 0x3,
            0xF0 => self.main_counter = val,
            _ => {}
        }
    }
}

fn mask_size(val: u64, size: u8) -> u64 { match size { 1 => val & 0xFF, 2 => val & 0xFFFF, 4 => val & 0xFFFF_FFFF, _ => val } }
fn merge_size(old: u64, data: u64, size: u8) -> u64 {
    match size { 1 => (old & !0xFF) | (data & 0xFF), 2 => (old & !0xFFFF) | (data & 0xFFFF), 4 => (old & !0xFFFF_FFFF) | (data & 0xFFFF_FFFF), _ => data }
}
