//! Abstracción de dispositivos virtuales (I/O ports y MMIO).

use capsule_devices::Uart16550;

pub trait VirtualDevice: Send {
    fn name(&self) -> &str;
    fn handles_port(&self, port: u16) -> bool;
    fn read_io(&mut self, port: u16, size: u8) -> u32;
    fn write_io(&mut self, port: u16, data: u32, size: u8);
}

/// Implementación del trait VirtualDevice para el controlador serie UART 16550 estándar.
impl VirtualDevice for Uart16550 {
    fn name(&self) -> &str {
        "UART16550_COM1"
    }

    fn handles_port(&self, port: u16) -> bool {
        self.handles_port(port)
    }

    fn read_io(&mut self, port: u16, _size: u8) -> u32 {
        self.read_io(port) as u32
    }

    fn write_io(&mut self, port: u16, data: u32, _size: u8) {
        self.write_io(port, (data & 0xFF) as u8);
    }
}

/// Dispositivo básico de consola / puerto de depuración en puerto simple (ej. puerto 0xE9).
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
