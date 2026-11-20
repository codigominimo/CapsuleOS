//! Abstracción de dispositivos virtuales (I/O ports).

use capsule_devices::{IdeController, PciBus, Pic8259, Pit8254, RtcCmos, Uart16550};
use std::any::Any;

pub trait VirtualDevice: Send + Any {
    fn name(&self) -> &str;
    fn handles_port(&self, port: u16) -> bool;
    fn read_io(&mut self, port: u16, size: u8) -> u32;
    fn write_io(&mut self, port: u16, data: u32, size: u8);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Avanza el estado interno del dispositivo (timers, contadores, etc.).
    /// `cycles` representa pasos/ciclos aproximados de la vCPU desde el último tick.
    fn tick(&mut self, _cycles: u64) {}
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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementación del trait VirtualDevice para el controlador ATA/IDE PIO primario.
impl VirtualDevice for IdeController {
    fn name(&self) -> &str {
        "IDE_PRIMARY"
    }

    fn handles_port(&self, port: u16) -> bool {
        self.handles_port(port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        IdeController::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        IdeController::write_io(self, port, data, size);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementación del trait VirtualDevice para el PIC 8259 dual (maestro/esclavo).
impl VirtualDevice for Pic8259 {
    fn name(&self) -> &str {
        "PIC8259"
    }

    fn handles_port(&self, port: u16) -> bool {
        Pic8259::handles_port(self, port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        Pic8259::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        Pic8259::write_io(self, port, data, size);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementación del trait VirtualDevice para el PIT 8254.
impl VirtualDevice for Pit8254 {
    fn name(&self) -> &str {
        "PIT8254"
    }

    fn handles_port(&self, port: u16) -> bool {
        Pit8254::handles_port(self, port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        Pit8254::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        Pit8254::write_io(self, port, data, size);
    }

    fn tick(&mut self, cycles: u64) {
        let _ = Pit8254::tick(self, cycles);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementación del trait VirtualDevice para el RTC/CMOS MC146818.
impl VirtualDevice for RtcCmos {
    fn name(&self) -> &str {
        "RTC_CMOS"
    }

    fn handles_port(&self, port: u16) -> bool {
        RtcCmos::handles_port(self, port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        RtcCmos::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        RtcCmos::write_io(self, port, data, size);
    }

    fn tick(&mut self, _cycles: u64) {
        RtcCmos::tick(self);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementación del trait VirtualDevice para el bus PCI (configuración mechanism #1).
/// El sondeo de dispositivos PCI con acceso a memoria guest (p.ej. virtio-net/NAT) se realiza
/// explícitamente desde el bucle de la VM (`PciBus::poll_devices`), ya que requiere `GuestMemory`.
impl VirtualDevice for PciBus {
    fn name(&self) -> &str {
        "PCI_BUS"
    }

    fn handles_port(&self, port: u16) -> bool {
        PciBus::handles_port(self, port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        PciBus::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        PciBus::write_io(self, port, data, size);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
