pub mod cpu;
pub mod memory;
pub mod os;
pub mod whpx;

pub use cpu::CpuInfo;
pub use memory::MemoryInfo;
pub use os::OsInfo;
pub use whpx::{detect_whpx, WhpxStatus};

#[derive(Debug, Clone)]
pub struct HostInfo {
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub whpx: WhpxStatus,
}

impl HostInfo {
    pub fn detect() -> Self {
        Self {
            os: OsInfo::detect(),
            cpu: CpuInfo::detect(),
            memory: MemoryInfo::detect(),
            whpx: detect_whpx(),
        }
    }
}
