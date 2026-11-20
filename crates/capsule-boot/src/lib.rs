//! # capsule-boot
//!
//! Implementación del protocolo de arranque directo de Linux x86 (bzImage direct boot)
//! y configuración de estructuras arquitecturales del sistema (Zero-Page, E820 y tablas de páginas).

pub mod bootparams;
pub mod loader;
pub mod pagetables;

pub use bootparams::{BootParams, E820Entry, SetupHeader, HDRS_MAGIC};
pub use loader::{LinuxBootInfo, LinuxKernelLoader, BOOT_PARAMS_ADDR, CMDLINE_ADDR, KERNEL_LOAD_ADDR};
pub use pagetables::{setup_identity_pagetables, PDPT_ADDRESS, PD_ADDRESS, PML4_ADDRESS};
