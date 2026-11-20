//! Abstracción de dispositivos virtuales (I/O ports y MMIO).

pub trait VirtualDevice: Send {
    fn name(&self) -> &str;
    fn handles_port(&self, port: u16) -> bool;
    fn read_io(&mut self, port: u16, size: u8) -> u32;
    fn write_io(&mut self, port: u16, data: u32, size: u8);
}

/// Dispositivo básico de consola / puerto de depuración (ej. puerto 0xE9 o COM1 0x3F8).
pub struct DebugConsoleDevice {
    pub name: String,
    pub port: u16,
    pub output_buffer: Vec<u8>,
}

impl DebugConsoleDevice {
    pub fn new(port: u16) -> Self {
        Self {
            name: "DebugConsole".to_string(),
            port,
            output_buffer: Vec::new(),
        }
    }
}

impl VirtualDevice for DebugConsoleDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn handles_port(&self, port: u16) -> bool {
        self.port == port
    }

    fn read_io(&mut self, _port: u16, _size: u8) -> u32 {
        0xFF
    }

    fn write_io(&mut self, _port: u16, data: u32, _size: u8) {
        let byte = (data & 0xFF) as u8;
        self.output_buffer.push(byte);
    }
}
