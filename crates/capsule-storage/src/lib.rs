//! # capsule-storage
//!
//! Abstracciones de almacenamiento en bloque y formatos de imagen de disco (RAW, VHD fixed y overlay COW).

pub mod cow;
pub mod device;
pub mod error;
pub mod memory;
pub mod raw;
pub mod vhd;

pub use cow::CowOverlayDisk;
pub use device::BlockDevice;
pub use error::StorageError;
pub use memory::MemoryDisk;
pub use raw::RawDiskImage;
pub use vhd::VhdFixedDisk;

pub const DEFAULT_SECTOR_SIZE: u32 = 512;
