//! Detección dinámica de Windows Hypervisor Platform (WHPX).

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhpxStatus {
    /// WHPX está instalado, habilitado y el hipervisor está activo y listo para virtualización por hardware.
    Available,
    /// La biblioteca WinHvPlatform.dll está presente, pero el hipervisor no está activo
    /// (e.g. la característica opcional HypervisorPlatform no está habilitada en Windows o VT-x en BIOS).
    FeatureNotEnabled,
    /// WinHvPlatform.dll no está disponible en este sistema operativo.
    NotInstalled,
}

impl WhpxStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, WhpxStatus::Available)
    }

    pub fn description(&self) -> &'static str {
        match self {
            WhpxStatus::Available => "WHPX Disponible (Aceleración por Hardware Activa)",
            WhpxStatus::FeatureNotEnabled => {
                "WinHvPlatform.dll detectada, pero el hipervisor no está activo (Requiere 'Plataforma de hipervisor de Windows')"
            }
            WhpxStatus::NotInstalled => "WinHvPlatform.dll no encontrada en el sistema",
        }
    }
}

pub fn detect_whpx() -> WhpxStatus {
    unsafe {
        let dll_name: Vec<u16> = "WinHvPlatform.dll\0".encode_utf16().collect();
        let module = LoadLibraryW(dll_name.as_ptr());

        if module.is_null() {
            return WhpxStatus::NotInstalled;
        }

        // WHvCapabilityCodeHypervisorPresent = 0
        const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 = 0;

        let fn_name = b"WHvGetCapability\0";
        let proc = GetProcAddress(module, fn_name.as_ptr());

        let status = if let Some(proc_addr) = proc {
            type WHvGetCapabilityFn = unsafe extern "system" fn(
                capability_code: u32,
                capability_buffer: *mut u32,
                buffer_size: u32,
                written_size: *mut u32,
            ) -> i32;

            let get_cap: WHvGetCapabilityFn = std::mem::transmute(proc_addr);
            let mut present: u32 = 0;
            let mut written: u32 = 0;

            let hr = get_cap(
                WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT,
                &mut present,
                std::mem::size_of::<u32>() as u32,
                &mut written,
            );

            // S_OK = 0
            if hr == 0 && present != 0 {
                WhpxStatus::Available
            } else {
                WhpxStatus::FeatureNotEnabled
            }
        } else {
            WhpxStatus::FeatureNotEnabled
        };

        FreeLibrary(module);
        status
    }
}
