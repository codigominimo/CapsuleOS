//! # capsule-vmm
//!
//! Capa de abstracción del Monitor de Máquinas Virtuales (VMM) de CapsuleOS.
//! Define los traits y tipos unificados que implementarán tanto el backend de aceleración
//! por hardware (capsule-whpx) como el backend de emulación por software (capsule-emulator).

pub mod config;
pub mod state;
pub mod traits;

pub use config::VmConfig;
pub use state::{VcpuExit, VmStatus};
pub use traits::{Error, VirtualCpu, VmBackend};
