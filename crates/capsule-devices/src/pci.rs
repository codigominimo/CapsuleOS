use capsule_memory::GuestMemory;
use std::any::Any;

pub const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Dispositivo/función PCI conectable al bus de configuración (Mecanismo #1).
pub trait PciDevice: Send + Any {
    /// (bus, device, function)
    fn bdf(&self) -> (u8, u8, u8);
    fn config_read(&mut self, offset: u8, size: u8) -> u32;
    fn config_write(&mut self, offset: u8, value: u32, size: u8);
    fn handles_io_port(&self, port: u16) -> bool;
    fn io_read(&mut self, port: u16, size: u8) -> u32;
    fn io_write(&mut self, port: u16, data: u32, size: u8);
    fn handles_mmio(&self, _addr: u64) -> bool { false }
    fn mmio_read(&mut self, _addr: u64, _size: u8) -> u64 { 0 }
    fn mmio_write(&mut self, _addr: u64, _data: u64, _size: u8) {}
    fn poll(&mut self, _mem: &mut GuestMemory) {}
    fn irq_level(&self) -> bool { false }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Bus PCI (Mecanismo #1 de configuración: 0xCF8/0xCFC).
///
/// Soporta direccionamiento de configuración y enrutamiento a dispositivos PCI registrados.
/// Sin dispositivo para la BDF seleccionada, las lecturas devuelven 0xFFFF_FFFF (dispositivo inexistente).
pub struct PciBus {
    config_address: u32,
    devices: Vec<Box<dyn PciDevice>>,
}

impl Default for PciBus {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PciBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PciBus")
            .field("config_address", &format_args!("0x{:08X}", self.config_address))
            .field("devices", &self.devices.len())
            .finish()
    }
}

impl PciBus {
    pub fn new() -> Self {
        Self {
            config_address: 0,
            devices: Vec::new(),
        }
    }

    pub fn attach_device(&mut self, device: Box<dyn PciDevice>) {
        self.devices.push(device);
    }

    pub fn devices_mut(&mut self) -> &mut [Box<dyn PciDevice>] {
        &mut self.devices
    }

    pub fn devices(&self) -> &[Box<dyn PciDevice>] {
        &self.devices
    }

    pub fn poll_devices(&mut self, mem: &mut GuestMemory) {
        for dev in &mut self.devices {
            dev.poll(mem);
        }
    }

    /// Nivel IRQ agregado (OR) de dispositivos PCI que señalan INTx.
    /// (El enrutamiento específico de INTx->IRQ de plataforma se gestiona en la VM.)
    pub fn any_irq_level(&self) -> bool {
        self.devices.iter().any(|d| d.irq_level())
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0xCF8..=0xCFB | 0xCFC..=0xCFF)
            || self.devices.iter().any(|d| d.handles_io_port(port))
    }

    pub fn handles_mmio(&self, addr: u64) -> bool {
        self.devices.iter().any(|d| d.handles_mmio(addr))
    }

    pub fn mmio_read(&mut self, addr: u64, size: u8) -> u64 {
        for dev in &mut self.devices {
            if dev.handles_mmio(addr) { return dev.mmio_read(addr, size); }
        }
        0
    }

    pub fn mmio_write(&mut self, addr: u64, data: u64, size: u8) {
        for dev in &mut self.devices {
            if dev.handles_mmio(addr) { dev.mmio_write(addr, data, size); return; }
        }
    }

    #[inline]
    fn config_enabled(&self) -> bool {
        (self.config_address & 0x8000_0000) != 0
    }

    #[inline]
    fn size_bytes(size: u8) -> u8 {
        match size {
            1 | 2 | 4 => size,
            _ => 1,
        }
    }

    #[inline]
    fn selected_bdf(&self) -> (u8, u8, u8) {
        let addr = self.config_address;
        let bus = ((addr >> 16) & 0xFF) as u8;
        let dev = ((addr >> 11) & 0x1F) as u8;
        let func = ((addr >> 8) & 0x07) as u8;
        (bus, dev, func)
    }

    #[inline]
    fn selected_register_offset(&self, data_port: u16) -> u8 {
        let base = (self.config_address & 0xFC) as u8;
        let byte_in_dword = (self.config_address & 0x03) as u8;
        let data_off = (data_port - PCI_CONFIG_DATA) as u8;
        base.wrapping_add(byte_in_dword).wrapping_add(data_off)
    }

    fn find_device_mut(&mut self, bdf: (u8, u8, u8)) -> Option<&mut Box<dyn PciDevice>> {
        self.devices.iter_mut().find(|d| d.bdf() == bdf)
    }

    pub fn read_io(&mut self, port: u16, size: u8) -> u32 {
        let size = Self::size_bytes(size);
        match port {
            0xCF8..=0xCFB => {
                let shift = ((port - 0xCF8) * 8) as u32;
                let mask = match size {
                    1 => 0xFFu32,
                    2 => 0xFFFFu32,
                    4 => 0xFFFF_FFFFu32,
                    _ => 0xFFu32,
                };
                (self.config_address >> shift) & mask
            }
            0xCFC..=0xCFF => {
                if !self.config_enabled() {
                    return match size {
                        1 => 0xFF,
                        2 => 0xFFFF,
                        _ => 0xFFFF_FFFF,
                    };
                }
                let bdf = self.selected_bdf();
                let reg_off = self.selected_register_offset(port);
                if let Some(dev) = self.find_device_mut(bdf) {
                    return dev.config_read(reg_off, size);
                }
                match size {
                    1 => 0xFF,
                    2 => 0xFFFF,
                    _ => 0xFFFF_FFFF,
                }
            }
            _ => {
                for dev in &mut self.devices {
                    if dev.handles_io_port(port) {
                        return dev.io_read(port, size);
                    }
                }
                0xFF
            }
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, size: u8) {
        let size = Self::size_bytes(size);
        match port {
            0xCF8..=0xCFB => {
                let shift = ((port - 0xCF8) * 8) as u32;
                let mask = match size {
                    1 => 0xFFu32 << shift,
                    2 => 0xFFFFu32 << shift,
                    4 => 0xFFFF_FFFFu32,
                    _ => 0xFFu32 << shift,
                };
                self.config_address = (self.config_address & !mask) | ((val << shift) & mask);
            }
            0xCFC..=0xCFF => {
                if !self.config_enabled() {
                    return;
                }
                let bdf = self.selected_bdf();
                let reg_off = self.selected_register_offset(port);
                if let Some(dev) = self.find_device_mut(bdf) {
                    dev.config_write(reg_off, val, size);
                }
            }
            _ => {
                for dev in &mut self.devices {
                    if dev.handles_io_port(port) {
                        dev.io_write(port, val, size);
                        return;
                    }
                }
            }
        }
    }
}
