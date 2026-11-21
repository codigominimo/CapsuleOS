//! Abstracción de dispositivos virtuales (I/O ports).

use capsule_devices::{BochsVbe, Hpet, I8042Ps2Controller, IdeController, IoApic, LocalApic, PciBus, Pic8259, Pit8254, RtcCmos, Uart16550};
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

    // MMIO (Memory-Mapped I/O). Por defecto no manejado.
    fn handles_mmio(&self, _addr: u64) -> bool { false }
    fn read_mmio(&mut self, _addr: u64, _size: u8) -> u64 { 0 }
    fn write_mmio(&mut self, _addr: u64, _data: u64, _size: u8) {}
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
    fn name(&self) -> &str { "PCI_BUS" }
    fn handles_port(&self, port: u16) -> bool { PciBus::handles_port(self, port) }
    fn read_io(&mut self, port: u16, size: u8) -> u32 { PciBus::read_io(self, port, size) }
    fn write_io(&mut self, port: u16, data: u32, size: u8) { PciBus::write_io(self, port, data, size); }
    fn handles_mmio(&self, addr: u64) -> bool { PciBus::handles_mmio(self, addr) }
    fn read_mmio(&mut self, addr: u64, size: u8) -> u64 { PciBus::mmio_read(self, addr, size) }
    fn write_mmio(&mut self, addr: u64, data: u64, size: u8) { PciBus::mmio_write(self, addr, data, size); }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}


/// Implementacion del trait VirtualDevice para el adaptador grafico Bochs VBE (puertos 0x01CE/0x01CF).
/// El framebuffer lineal (LFB) reside en memoria fisica guest; la VM/GUI lo lee desde `GuestMemory`.
impl VirtualDevice for BochsVbe {
    fn name(&self) -> &str {
        "BOCHS_VBE"
    }

    fn handles_port(&self, port: u16) -> bool {
        BochsVbe::handles_port(self, port)
    }

    fn read_io(&mut self, port: u16, size: u8) -> u32 {
        BochsVbe::read_io(self, port, size)
    }

    fn write_io(&mut self, port: u16, data: u32, size: u8) {
        BochsVbe::write_io(self, port, data, size);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Implementacion del trait VirtualDevice para el controlador PS/2 i8042 (teclado/raton, 0x60/0x64).
impl VirtualDevice for I8042Ps2Controller {
    fn name(&self) -> &str { "I8042_PS2" }
    fn handles_port(&self, port: u16) -> bool { I8042Ps2Controller::handles_port(self, port) }
    fn read_io(&mut self, port: u16, size: u8) -> u32 { I8042Ps2Controller::read_io(self, port, size) }
    fn write_io(&mut self, port: u16, data: u32, size: u8) { I8042Ps2Controller::write_io(self, port, data, size); }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl VirtualDevice for LocalApic {
    fn name(&self) -> &str { "LAPIC" }
    fn handles_port(&self, _port: u16) -> bool { false }
    fn read_io(&mut self, _port: u16, _size: u8) -> u32 { 0 }
    fn write_io(&mut self, _port: u16, _data: u32, _size: u8) {}
    fn handles_mmio(&self, addr: u64) -> bool { LocalApic::handles_mmio(self, addr) }
    fn read_mmio(&mut self, addr: u64, size: u8) -> u64 { LocalApic::read_mmio(self, addr, size) }
    fn write_mmio(&mut self, addr: u64, data: u64, size: u8) { LocalApic::write_mmio(self, addr, data, size); }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl VirtualDevice for IoApic {
    fn name(&self) -> &str { "IOAPIC" }
    fn handles_port(&self, _port: u16) -> bool { false }
    fn read_io(&mut self, _port: u16, _size: u8) -> u32 { 0 }
    fn write_io(&mut self, _port: u16, _data: u32, _size: u8) {}
    fn handles_mmio(&self, addr: u64) -> bool { IoApic::handles_mmio(self, addr) }
    fn read_mmio(&mut self, addr: u64, size: u8) -> u64 { IoApic::read_mmio(self, addr, size) }
    fn write_mmio(&mut self, addr: u64, data: u64, size: u8) { IoApic::write_mmio(self, addr, data, size); }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl VirtualDevice for Hpet {
    fn name(&self) -> &str { "HPET" }
    fn handles_port(&self, _port: u16) -> bool { false }
    fn read_io(&mut self, _port: u16, _size: u8) -> u32 { 0 }
    fn write_io(&mut self, _port: u16, _data: u32, _size: u8) {}
    fn handles_mmio(&self, addr: u64) -> bool { Hpet::handles_mmio(self, addr) }
    fn read_mmio(&mut self, addr: u64, size: u8) -> u64 { Hpet::read_mmio(self, addr, size) }
    fn write_mmio(&mut self, addr: u64, data: u64, size: u8) { Hpet::write_mmio(self, addr, data, size); }
    fn tick(&mut self, cycles: u64) { Hpet::tick(self, cycles); }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
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
