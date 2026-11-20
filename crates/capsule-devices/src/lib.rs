//! # capsule-devices
//!
//! Emulación de dispositivos periféricos y buses de la plataforma de PC (UART serie 16550, temporizadores, etc.).

pub mod uart16550;

pub use uart16550::{Uart16550, COM1_BASE};
