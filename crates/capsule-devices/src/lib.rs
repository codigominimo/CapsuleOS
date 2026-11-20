//! # capsule-devices
//!
//! Emulación de dispositivos periféricos y buses de la plataforma de PC
//! (UART serie 16550, controlador ATA/IDE PIO primario, etc.).

pub mod ide;
pub mod uart16550;

pub use ide::{IdeController, PRIMARY_CTRL, PRIMARY_IO_BASE, PRIMARY_IO_END};
pub use uart16550::{Uart16550, COM1_BASE};
