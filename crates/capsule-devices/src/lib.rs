//! # capsule-devices
//!
//! Emulación de dispositivos periféricos y buses de la plataforma de PC
//! (UART 16550, ATA/IDE PIO, PIC 8259, PIT 8254, RTC/CMOS, bus PCI, virtio-net).

pub mod ide;
pub mod pci;
pub mod pic;
pub mod platform;
pub mod uart16550;
pub mod virtio_net;

pub use ide::{IdeController, PRIMARY_CTRL, PRIMARY_IO_BASE, PRIMARY_IO_END};
pub use pci::{PciBus, PciDevice, PCI_CONFIG_ADDRESS, PCI_CONFIG_DATA};
pub use pic::Pic8259;
pub use platform::{Pit8254, RtcCmos};
pub use uart16550::{Uart16550, COM1_BASE};
pub use virtio_net::VirtioNetPci;
