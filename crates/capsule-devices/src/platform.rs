use std::time::{SystemTime, UNIX_EPOCH};

pub const PIT_CH0: u16 = 0x40;
pub const PIT_CH1: u16 = 0x41;
pub const PIT_CH2: u16 = 0x42;
pub const PIT_CMD: u16 = 0x43;

#[derive(Debug, Clone, Copy)]
struct Channel {
    reload: u16,
    counter: u16,
    latched: Option<u16>,
    access_mode: u8, // 1=lo,2=hi,3=lo/hi
    access_phase: u8, // 0 expect lo/hi depending, 1 expect hi
    operating_mode: u8,
    bcd: bool,
    output: bool,
    gate: bool,
    reload_pending: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            reload: 0,
            counter: 0,
            latched: None,
            access_mode: 0,
            access_phase: 0,
            operating_mode: 0,
            bcd: false,
            output: true,
            gate: true,
            reload_pending: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pit8254 {
    channels: [Channel; 3],
    irq0_pulse: bool,
}

impl Pit8254 {
    pub fn new() -> Self {
        Self { channels: [Channel::default(), Channel::default(), Channel::default()], irq0_pulse: false }
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, PIT_CH0 | PIT_CH1 | PIT_CH2 | PIT_CMD)
    }

    /// Avanza el PIT `ticks` ciclos de reloj (aproximados). Retorna true si IRQ0 (canal 0) se activó.
    pub fn tick(&mut self, ticks: u64) -> bool {
        self.irq0_pulse = false;
        if ticks == 0 { return false; }
        let ch = &mut self.channels[0];
        if !ch.gate { return false; }

        let steps = ticks.min(4096);
        for _ in 0..steps {
            if ch.reload_pending {
                ch.counter = if ch.reload == 0 { 0 } else { ch.reload };
                ch.reload_pending = false;
            }
            if ch.counter == 0 {
                ch.counter = if ch.reload == 0 { 0 } else { ch.reload };
            } else {
                ch.counter = ch.counter.wrapping_sub(1);
            }
            match ch.operating_mode & 0x07 {
                0 => { // interrupt on terminal count
                    if ch.counter == 0 { ch.output = true; self.irq0_pulse = true; }
                }
                2 => { // rate generator
                    if ch.counter == 1 { self.irq0_pulse = true; ch.output = false; } else { ch.output = true; }
                }
                3 => { // square wave (simplificado)
                    if ch.counter == 0 { self.irq0_pulse = true; ch.output = !ch.output; }
                }
                _ => {
                    if ch.counter == 0 { self.irq0_pulse = true; }
                }
            }
        }
        self.irq0_pulse
    }

    /// Valor actual del contador del canal indicado (para pruebas/diagnóstico).
    pub fn channel_counter(&self, channel: usize) -> Option<u16> {
        self.channels.get(channel).map(|c| c.counter)
    }

    pub fn read_io(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            PIT_CH0 => self.read_channel(0) as u32,
            PIT_CH1 => self.read_channel(1) as u32,
            PIT_CH2 => self.read_channel(2) as u32,
            PIT_CMD => 0xFF,
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, _size: u8) {
        let v = (val & 0xFF) as u8;
        match port {
            PIT_CH0 => self.write_channel(0, v),
            PIT_CH1 => self.write_channel(1, v),
            PIT_CH2 => self.write_channel(2, v),
            PIT_CMD => self.write_cmd(v),
            _ => {}
        }
    }

    fn write_cmd(&mut self, v: u8) {
        let ch_idx = ((v >> 6) & 0x03) as usize;
        if ch_idx == 3 { return; } // read-back no implementado
        let access = (v >> 4) & 0x03;
        let mode = (v >> 1) & 0x07;
        let bcd = v & 0x01 != 0;
        let ch = &mut self.channels[ch_idx];
        if access == 0 {
            // latch count
            let val = ch.counter;
            ch.latched = Some(val);
            ch.access_phase = 0;
        } else {
            ch.access_mode = access;
            ch.access_phase = 0;
            ch.operating_mode = mode & 0x07;
            ch.bcd = bcd;
            ch.reload_pending = false;
        }
    }

    fn read_channel(&mut self, idx: usize) -> u8 {
        let ch = &mut self.channels[idx];
        let value = ch.latched.unwrap_or(ch.counter);
        match ch.access_mode {
            1 => { ch.latched = None; (value & 0xFF) as u8 }
            2 => { ch.latched = None; (value >> 8) as u8 }
            3 => {
                if ch.access_phase == 0 {
                    ch.access_phase = 1;
                    (value & 0xFF) as u8
                } else {
                    ch.access_phase = 0;
                    ch.latched = None;
                    (value >> 8) as u8
                }
            }
            _ => (value & 0xFF) as u8,
        }
    }

    fn write_channel(&mut self, idx: usize, v: u8) {
        let ch = &mut self.channels[idx];
        match ch.access_mode {
            1 => { ch.reload = (ch.reload & 0xFF00) | v as u16; ch.reload_pending = true; ch.output = ch.operating_mode == 0; }
            2 => { ch.reload = (ch.reload & 0x00FF) | ((v as u16) << 8); ch.reload_pending = true; ch.output = ch.operating_mode == 0; }
            3 => {
                if ch.access_phase == 0 {
                    ch.reload = (ch.reload & 0xFF00) | v as u16;
                    ch.access_phase = 1;
                } else {
                    ch.reload = (ch.reload & 0x00FF) | ((v as u16) << 8);
                    ch.access_phase = 0;
                    ch.reload_pending = true;
                    ch.output = ch.operating_mode == 0;
                }
            }
            _ => {}
        }
    }
}

impl Default for Pit8254 {
    fn default() -> Self { Self::new() }
}

/// RTC/CMOS MC146818 mínimo (puertos 0x70 índice, 0x71 datos).
#[derive(Debug, Clone)]
pub struct RtcCmos {
    index: u8,
    cmos: [u8; 128],
    nmi_disabled: bool,
}

impl RtcCmos {
    pub fn new() -> Self {
        let mut cmos = [0u8; 128];
        // Status B: 24h + binary mode por defecto
        cmos[0x0B] = 0x06;
        // Status D: VRT bit
        cmos[0x0D] = 0x80;
        let mut rtc = Self { index: 0, cmos, nmi_disabled: false };
        rtc.update_time();
        rtc
    }

    pub fn handles_port(&self, port: u16) -> bool { matches!(port, 0x70 | 0x71) }

    fn update_time(&mut self) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let s = (secs % 60) as u8;
        let m = ((secs / 60) % 60) as u8;
        let h = ((secs / 3600) % 24) as u8;
        let days = secs / 86400;
        let (year, month, day) = days_to_ymd(days);
        self.cmos[0x00] = s;
        self.cmos[0x02] = m;
        self.cmos[0x04] = h;
        self.cmos[0x07] = day;
        self.cmos[0x08] = month;
        self.cmos[0x09] = (year % 100) as u8;
        self.cmos[0x32] = (year / 100) as u8;
        self.cmos[0x0A] &= 0x7F; // UIP clear
    }

    pub fn tick(&mut self) {
        self.update_time();
    }

    pub fn read_io(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            0x70 => self.index as u32,
            0x71 => {
                let idx = (self.index & 0x7F) as usize;
                if matches!(idx, 0x00..=0x0D) { self.update_time(); }
                self.cmos[idx] as u32
            }
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, _size: u8) {
        let v = (val & 0xFF) as u8;
        match port {
            0x70 => { self.index = v & 0x7F; self.nmi_disabled = v & 0x80 != 0; }
            0x71 => { let idx = (self.index & 0x7F) as usize; self.cmos[idx] = v; }
            _ => {}
        }
    }
}

impl Default for RtcCmos {
    fn default() -> Self { Self::new() }
}

fn is_leap(y: u64) -> bool { (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) }
fn days_in_month(y: u64, m: u64) -> u64 {
    match m { 1|3|5|7|8|10|12 => 31, 4|6|9|11 => 30, 2 => if is_leap(y) { 29 } else { 28 }, _ => 30 }
}
fn days_to_ymd(mut days: u64) -> (u64, u8, u8) {
    let mut y = 1970u64;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if days < diy { break; }
        days -= diy; y += 1;
        if y > 2099 { break; }
    }
    let mut m = 1u64;
    loop {
        let dim = days_in_month(y, m);
        if days < dim { break; }
        days -= dim; m += 1;
        if m > 12 { m = 12; break; }
    }
    (y, m as u8, (days + 1) as u8)
}
