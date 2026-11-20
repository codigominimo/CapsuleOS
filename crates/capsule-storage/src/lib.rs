//! # capsule-storage
//!
//! Abstracciones de almacenamiento en bloque y formatos de imagen de disco (RAW, VHD fixed, y futuros VHDX/QCOW2).

pub mod device;
pub mod error;
pub mod memory;
pub mod raw;
pub mod vhd;

pub use device::BlockDevice;
pub use error::StorageError;
pub use memory::MemoryDisk;
pub use raw::RawDiskImage;
pub use vhd::VhdFixedDisk;

pub const DEFAULT_SECTOR_SIZE: u32 = 512;
