//! Detección del procesador (CPU) host.

use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub vendor: String,
    pub brand_name: String,
    pub logical_cores: u32,
    pub has_vmx: bool, // Intel VT-x (bare metal)
    pub has_svm: bool, // AMD SVM (bare metal)
    pub hypervisor_present: bool, // CPUID Leaf 1 ECX bit 31 (Hyper-V / Hipervisor activo)
}

impl CpuInfo {
    pub fn detect() -> Self {
        let logical_cores = unsafe {
            let mut sys_info: SYSTEM_INFO = std::mem::zeroed();
            GetSystemInfo(&mut sys_info);
            sys_info.dwNumberOfProcessors
        };

        let vendor = detect_vendor();
        let brand_name = detect_brand_name();
        let (has_vmx, has_svm, hypervisor_present) = detect_virtualization_instructions(&vendor);

        Self {
            vendor,
            brand_name,
            logical_cores,
            has_vmx,
            has_svm,
            hypervisor_present,
        }
    }

    pub fn virtualization_supported(&self) -> bool {
        self.has_vmx || self.has_svm || self.hypervisor_present
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_vendor() -> String {
    use std::arch::x86_64::__cpuid;
    let res = __cpuid(0);
    let mut bytes = [0u8; 12];
    bytes[0..4].copy_from_slice(&res.ebx.to_le_bytes());
    bytes[4..8].copy_from_slice(&res.edx.to_le_bytes());
    bytes[8..12].copy_from_slice(&res.ecx.to_le_bytes());
    String::from_utf8_lossy(&bytes).trim().to_string()
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_vendor() -> String {
    "Unknown Vendor".to_string()
}

#[cfg(target_arch = "x86_64")]
fn detect_brand_name() -> String {
    use std::arch::x86_64::__cpuid;
    let max_ext = __cpuid(0x80000000).eax;
    if max_ext < 0x80000004 {
        return "Generic x86_64 CPU".to_string();
    }

    let mut bytes = [0u8; 48];
    for i in 0..3 {
        let leaf = 0x80000002 + i;
        let res = __cpuid(leaf);
        let offset = (i as usize) * 16;
        bytes[offset..offset + 4].copy_from_slice(&res.eax.to_le_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&res.ebx.to_le_bytes());
        bytes[offset + 8..offset + 12].copy_from_slice(&res.ecx.to_le_bytes());
        bytes[offset + 12..offset + 16].copy_from_slice(&res.edx.to_le_bytes());
    }

    let raw_name = String::from_utf8_lossy(&bytes);
    let trimmed = raw_name.trim_matches(char::from(0)).trim();
    if trimmed.is_empty() {
        "x86_64 CPU".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_brand_name() -> String {
    "Generic CPU".to_string()
}

#[cfg(target_arch = "x86_64")]
fn detect_virtualization_instructions(vendor: &str) -> (bool, bool, bool) {
    use std::arch::x86_64::__cpuid;

    let leaf1 = __cpuid(1);
    let hypervisor_present = (leaf1.ecx & (1 << 31)) != 0;
    let mut has_vmx = (leaf1.ecx & (1 << 5)) != 0;
    let mut has_svm = false;

    // AMD SVM: Leaf 0x80000001, ECX bit 2
    let max_ext = __cpuid(0x80000000).eax;
    if max_ext >= 0x80000001 {
        let ext_leaf1 = __cpuid(0x80000001);
        has_svm = (ext_leaf1.ecx & (1 << 2)) != 0;
    }

    if vendor == "GenuineIntel" && (leaf1.ecx & (1 << 5)) != 0 {
        has_vmx = true;
    }

    (has_vmx, has_svm, hypervisor_present)
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_virtualization_instructions(_vendor: &str) -> (bool, bool, bool) {
    (false, false, false)
}
