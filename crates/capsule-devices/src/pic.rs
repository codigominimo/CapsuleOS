use std::fmt;

pub const PIC1_CMD: u16 = 0x20;
pub const PIC1_DATA: u16 = 0x21;
pub const PIC2_CMD: u16 = 0xA0;
pub const PIC2_DATA: u16 = 0xA1;

#[derive(Clone, Copy)]
struct Pic {
    imr: u8,   // Interrupt Mask Register (1 = masked)
    irr: u8,   // Interrupt Request Register
    isr: u8,   // In-Service Register
    icw_step: u8,
    icw1: u8,
    icw2: u8,  // vector base
    icw3: u8,
    icw4: u8,
    read_isr: bool, // OCW3: read ISR instead of IRR
    auto_eoi: bool,
    special_mask: bool,
}

impl Default for Pic {
    fn default() -> Self {
        Self {
            imr: 0xFF,
            irr: 0,
            isr: 0,
            icw_step: 0,
            icw1: 0,
            icw2: 0,
            icw3: 0,
            icw4: 0,
            read_isr: false,
            auto_eoi: false,
            special_mask: false,
        }
    }
}

impl fmt::Debug for Pic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pic")
            .field("imr", &format_args!("0x{:02X}", self.imr))
            .field("irr", &format_args!("0x{:02X}", self.irr))
            .field("isr", &format_args!("0x{:02X}", self.isr))
            .field("icw2", &format_args!("0x{:02X}", self.icw2))
            .finish()
    }
}

impl Pic {
    fn highest_pending_bit(mask: u8) -> Option<u8> {
        if mask == 0 { None } else { Some(mask.trailing_zeros() as u8) }
    }

    fn pending_for_service(&self) -> u8 {
        let mut req = self.irr & !self.imr;
        if !self.special_mask {
            if self.isr != 0 {
                if let Some(isr_pri) = Self::highest_pending_bit(self.isr) {
                    // bloquear IRQs de prioridad igual o menor (número >= isr_pri)
                    req &= (1u8 << isr_pri) - 1;
                }
            }
        }
        req
    }

    fn get_interrupt(&mut self) -> Option<u8> {
        let req = self.pending_for_service();
        let bit = Self::highest_pending_bit(req)?;
        self.irr &= !(1 << bit);
        if !self.auto_eoi {
            self.isr |= 1 << bit;
        }
        Some(self.icw2.wrapping_add(bit))
    }

    fn has_interrupt(&self) -> bool {
        self.pending_for_service() != 0
    }

    fn eoi_non_specific(&mut self) {
        if let Some(bit) = Self::highest_pending_bit(self.isr) {
            self.isr &= !(1 << bit);
        }
    }

    fn eoi_specific(&mut self, level: u8) {
        self.isr &= !(1 << (level & 0x07));
    }
}

/// Controlador PIC 8259 dual (maestro/esclavo) en configuración PC estándar.
#[derive(Debug, Clone)]
pub struct Pic8259 {
    master: Pic,
    slave: Pic,
    elcr_master: u8,
    elcr_slave: u8,
}

impl Pic8259 {
    pub fn new() -> Self {
        let mut master = Pic::default();
        let mut slave = Pic::default();
        // Bases por defecto PC (antes de reprogramación por guest)
        master.icw2 = 0x08;
        slave.icw2 = 0x70;
        Self { master, slave, elcr_master: 0, elcr_slave: 0 }
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, PIC1_CMD | PIC1_DATA | PIC2_CMD | PIC2_DATA | 0x4D0 | 0x4D1)
    }

    /// Establece (latch) una solicitud de IRQ (modelo simplificado edge-trigger).
    pub fn raise_irq(&mut self, irq: u8) {
        if irq >= 16 { return; }
        if irq < 8 {
            self.master.irr |= 1 << irq;
        } else {
            self.slave.irr |= 1 << (irq - 8);
        }
    }

    /// Limpia la solicitud latched de IRQ (útil para emular flancos/ack desde dispositivo).
    pub fn clear_irq_request(&mut self, irq: u8) {
        if irq >= 16 { return; }
        if irq < 8 {
            self.master.irr &= !(1 << irq);
        } else {
            self.slave.irr &= !(1 << (irq - 8));
        }
    }

    /// Fija el nivel lógico de la línea IRQ: en alto la mantiene solicitada, en bajo limpia la solicitud latched.
    /// Adecuado para wiring simple de dispositivos (p.ej. IDE IRQ14 mientras irq_pending && !nIEN).
    pub fn set_irq_level(&mut self, irq: u8, level: bool) {
        if level { self.raise_irq(irq); } else { self.clear_irq_request(irq); }
    }

    /// Retorna el vector de interrupción a inyectar (si hay), realizando acknowledge.
    /// No usado aún por la CPU software (sin IDT/interrupt injection), pero mantiene estado PIC coherente.
    pub fn get_interrupt(&mut self) -> Option<u8> {
        // Reflejar pendiente del esclavo como IRQ2 en maestro (nivel derivado)
        if self.slave.has_interrupt() {
            self.master.irr |= 1 << 2;
        } else if self.master.isr & (1 << 2) == 0 {
            // si no hay IRQ2 en servicio, no mantener solicitud fantasma por cascada
            self.master.irr &= !(1 << 2);
        }

        let master_req = self.master.pending_for_service();
        let bit = Pic::highest_pending_bit(master_req)?;

        if bit == 2 {
            // Cascada: atender esclavo
            self.master.irr &= !(1 << 2);
            if !self.master.auto_eoi {
                self.master.isr |= 1 << 2;
            }
            if let Some(vec) = self.slave.get_interrupt() {
                if !self.slave.has_interrupt() && self.master.isr & (1 << 2) == 0 {
                    self.master.irr &= !(1 << 2);
                }
                return Some(vec);
            } else {
                // Spurious esclavo: devolver vector spurious del esclavo (base+7)
                if !self.master.auto_eoi {
                    self.master.isr &= !(1 << 2);
                }
                return Some(self.slave.icw2.wrapping_add(7));
            }
        }

        self.master.get_interrupt()
    }

    pub fn has_pending_interrupt(&self) -> bool {
        if self.slave.has_interrupt() {
            let mut m = self.master;
            m.irr |= 1 << 2;
            return m.has_interrupt();
        }
        self.master.has_interrupt()
    }

    /// Acceso de solo lectura al IMR del maestro (para pruebas/diagnóstico).
    pub fn master_imr(&self) -> u8 {
        self.master.imr
    }

    /// Acceso de solo lectura al IMR del esclavo (para pruebas/diagnóstico).
    pub fn slave_imr(&self) -> u8 {
        self.slave.imr
    }

    /// Acceso de solo lectura al ISR del maestro.
    pub fn master_isr(&self) -> u8 {
        self.master.isr
    }

    /// Acceso de solo lectura al ISR del esclavo.
    pub fn slave_isr(&self) -> u8 {
        self.slave.isr
    }

    pub fn master_base(&self) -> u8 { self.master.icw2 }
    pub fn slave_base(&self) -> u8 { self.slave.icw2 }

    pub fn read_io(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            PIC1_CMD => (if self.master.read_isr { self.master.isr } else { self.master.irr }) as u32,
            PIC1_DATA => self.master.imr as u32,
            PIC2_CMD => (if self.slave.read_isr { self.slave.isr } else { self.slave.irr }) as u32,
            PIC2_DATA => self.slave.imr as u32,
            0x4D0 => self.elcr_master as u32,
            0x4D1 => self.elcr_slave as u32,
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, _size: u8) {
        let v = (val & 0xFF) as u8;
        match port {
            PIC1_CMD => self.write_cmd(true, v),
            PIC1_DATA => self.write_data(true, v),
            PIC2_CMD => self.write_cmd(false, v),
            PIC2_DATA => self.write_data(false, v),
            0x4D0 => self.elcr_master = v,
            0x4D1 => self.elcr_slave = v,
            _ => {}
        }
    }

    fn write_cmd(&mut self, master: bool, v: u8) {
        let pic = if master { &mut self.master } else { &mut self.slave };
        if v & 0x10 != 0 {
            // ICW1
            pic.icw1 = v;
            pic.icw_step = 1;
            pic.imr = 0;
            pic.isr = 0;
            pic.irr &= !(1 << 2); // limpiar cascada latched localmente
            pic.read_isr = false;
            pic.auto_eoi = false;
            pic.special_mask = false;
            if v & 0x01 == 0 {
                pic.icw4 = 0x01;
            }
            return;
        }
        if v & 0x08 != 0 {
            // OCW3
            if v & 0x02 != 0 {
                pic.read_isr = v & 0x01 != 0;
            }
            return;
        }
        // OCW2
        let eoi = v & 0x20 != 0;
        let specific = v & 0x40 != 0;
        if eoi {
            if specific {
                pic.eoi_specific(v & 0x07);
            } else {
                pic.eoi_non_specific();
            }
        }
    }

    fn write_data(&mut self, master: bool, v: u8) {
        let pic = if master { &mut self.master } else { &mut self.slave };
        match pic.icw_step {
            1 => { pic.icw2 = v; pic.icw_step = 2; }
            2 => {
                pic.icw3 = v;
                pic.icw_step = if pic.icw1 & 0x01 != 0 { 3 } else { 0 };
                if pic.icw_step == 0 { pic.auto_eoi = (pic.icw4 & 0x02) != 0; }
            }
            3 => { pic.icw4 = v; pic.icw_step = 0; pic.auto_eoi = (v & 0x02) != 0; }
            _ => {
                // OCW1: IMR
                pic.imr = v;
            }
        }
    }
}

impl Default for Pic8259 {
    fn default() -> Self { Self::new() }
}
