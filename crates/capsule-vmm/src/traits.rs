//! Traits fundamentales para la abstracción de hipervisores y emuladores.

use crate::config::VmConfig;
use crate::state::{VcpuExit, VmStatus};
use capsule_core::types::{GuestAddress, HostAddress};
use std::fmt;

#[derive(Debug)]
pub enum VmmError {
    InitializationFailed(String),
    MemoryMappingFailed(String),
    VcpuError(String),
    Unsupported,
}

impl fmt::Display for VmmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmmError::InitializationFailed(msg) => write!(f, "Error de inicialización de la VM: {}", msg),
            VmmError::MemoryMappingFailed(msg) => write!(f, "Error de mapeo de memoria: {}", msg),
            VmmError::VcpuError(msg) => write!(f, "Error de vCPU: {}", msg),
            VmmError::Unsupported => write!(f, "Operación no soportada por este backend"),
        }
    }
}

impl std::error::Error for VmmError {}

pub type Error = VmmError;

/// Abstracción de un procesador virtual.
pub trait VirtualCpu: Send {
    fn id(&self) -> usize;
    fn set_rip(&mut self, rip: u64) -> Result<(), Error>;
    fn rip(&self) -> Result<u64, Error>;
    fn run(&mut self) -> Result<VcpuExit, Error>;
}

/// Abstracción del backend del Monitor de Máquinas Virtuales (VMM).
pub trait VmBackend: Send {
    fn create_vm(&mut self, config: &VmConfig) -> Result<(), Error>;
    fn map_guest_memory(
        &mut self,
        gpa: GuestAddress,
        size: usize,
        hva: HostAddress,
        writable: bool,
        executable: bool,
    ) -> Result<(), Error>;
    fn create_vcpu(&mut self, id: usize) -> Result<Box<dyn VirtualCpu>, Error>;
    fn status(&self) -> VmStatus;
}
