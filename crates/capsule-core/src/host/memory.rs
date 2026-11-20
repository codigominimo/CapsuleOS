//! Detección de memoria RAM host.

use crate::types::ByteSize;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

#[derive(Debug, Clone)]
pub struct MemoryInfo {
    pub total_phys: ByteSize,
    pub avail_phys: ByteSize,
    pub used_phys: ByteSize,
    pub memory_load_percent: u32,
}

impl MemoryInfo {
    pub fn detect() -> Self {
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;

            if GlobalMemoryStatusEx(&mut status) != 0 {
                let total = status.ullTotalPhys;
                let avail = status.ullAvailPhys;
                let used = total.saturating_sub(avail);
                Self {
                    total_phys: ByteSize::from_bytes(total),
                    avail_phys: ByteSize::from_bytes(avail),
                    used_phys: ByteSize::from_bytes(used),
                    memory_load_percent: status.dwMemoryLoad,
                }
            } else {
                Self {
                    total_phys: ByteSize::from_bytes(0),
                    avail_phys: ByteSize::from_bytes(0),
                    used_phys: ByteSize::from_bytes(0),
                    memory_load_percent: 0,
                }
            }
        }
    }
}
