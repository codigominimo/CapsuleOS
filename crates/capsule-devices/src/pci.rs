pub const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Bus PCI simplificado (Mecanismo #1 de configuración: 0xCF8/0xCFC).
///
/// Modelo mínimo para M7: soporta direccionamiento de configuración y lecturas/escrituras
/// al espacio de configuración. Sin dispositivos PCI registrados devuelve 0xFFFF_FFFF
/// (dispositivo inexistente), permitiendo enumeración básica por parte del guest.
#[derive(Debug, Clone, Default)]
pub struct PciBus {
    config_address: u32,
}

impl PciBus {
    pub fn new() -> Self {
        Self { config_address: 0 }
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0xCF8..=0xCFB | 0xCFC..=0xCFF)
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

    pub fn read_io(&self, port: u16, size: u8) -> u32 {
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
                    return match size { 1 => 0xFF, 2 => 0xFFFF, _ => 0xFFFF_FFFF };
                }
                // Espacio de configuración vacío (sin funciones/dispositivos).
                let value: u32 = 0xFFFF_FFFF;
                let byte_off = ((self.config_address & 0x3) as u32) + (port - PCI_CONFIG_DATA) as u32;
                let shift = (byte_off.min(3) * 8) as u32;
                let mask = match size {
                    1 => 0xFFu32,
                    2 => 0xFFFFu32,
                    4 => 0xFFFF_FFFFu32,
                    _ => 0xFFu32,
                };
                (value >> shift) & mask
            }
            _ => 0xFF,
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
                // Sin dispositivos configurables en este modelo simplificado: escrituras ignoradas.
                let _ = (val, port, size);
            }
            _ => {}
        }
    }
}
