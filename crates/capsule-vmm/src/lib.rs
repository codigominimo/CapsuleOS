pub mod backend;
pub mod config;
pub mod device;
pub mod state;
pub mod traits;
pub mod vm;

pub use capsule_boot::LinuxBootInfo;
pub use config::{VmBackendKind, VmConfig};
pub use device::{DebugConsoleDevice, VirtualDevice};
pub use state::{VcpuExit, VmStatus};
pub use traits::{Error, VirtualCpu, VmBackend, VmmError};
pub use vm::VirtualMachine;
