//! # capsule-vmm
//!
//! Capa de abstracción y orquestación del Monitor de Máquinas Virtuales (VMM) de CapsuleOS.

pub mod config;
pub mod device;
pub mod state;
pub mod traits;
pub mod vm;

pub use config::VmConfig;
pub use device::{DebugConsoleDevice, VirtualDevice};
pub use state::{VcpuExit, VmStatus};
pub use traits::{Error, VirtualCpu, VmBackend, VmmError};
pub use vm::VirtualMachine;
