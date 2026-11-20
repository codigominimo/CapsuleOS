//! # capsule-core
//!
//! Núcleo fundacional de CapsuleOS: tipos primitivos de direcciones y memoria,
//! y utilidades de detección e introspección del sistema anfitrión (Windows, CPU, RAM, WHPX).

pub mod host;
pub mod types;

pub use host::{detect_whpx, CpuInfo, HostInfo, MemoryInfo, OsInfo, WhpxStatus};
pub use types::{ByteSize, GuestAddress, HostAddress};

pub const CAPSULE_VERSION: &str = env!("CARGO_PKG_VERSION");
