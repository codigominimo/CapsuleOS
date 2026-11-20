//! Detección del sistema operativo host.

use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ,
};

#[derive(Debug, Clone)]
pub struct OsInfo {
    pub os_name: String,
    pub build_number: String,
    pub display_version: String,
    pub architecture: &'static str,
}

impl OsInfo {
    pub fn detect() -> Self {
        let arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else {
            "unknown"
        };

        let mut os_name = "Windows (Unknown Version)".to_string();
        let mut build_number = "Unknown".to_string();
        let mut display_version = "".to_string();

        unsafe {
            let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0"
                .encode_utf16()
                .collect();
            let mut hkey: HKEY = std::mem::zeroed();

            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey.as_ptr(),
                0,
                KEY_READ,
                &mut hkey,
            ) == 0
            {
                if let Some(val) = read_reg_string(hkey, "ProductName") {
                    os_name = val;
                }
                if let Some(val) = read_reg_string(hkey, "CurrentBuildNumber") {
                    build_number = val;
                } else if let Some(val) = read_reg_string(hkey, "CurrentBuild") {
                    build_number = val;
                }
                if let Some(val) = read_reg_string(hkey, "DisplayVersion") {
                    display_version = val;
                }

                RegCloseKey(hkey);
            }
        }

        // Si es Windows 11, ProductName en el registro a menudo aún dice "Windows 10 Pro/Home"
        // pero la build es >= 22000.
        if let Ok(build) = build_number.parse::<u32>() {
            if build >= 22000 && os_name.contains("Windows 10") {
                os_name = os_name.replace("Windows 10", "Windows 11");
            }
        }

        Self {
            os_name,
            build_number,
            display_version,
            architecture: arch,
        }
    }
}

unsafe fn read_reg_string(hkey: HKEY, value_name: &str) -> Option<String> {
    let wide_name: Vec<u16> = value_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut reg_type: u32 = 0;
    let mut data_size: u32 = 0;

    let status = RegQueryValueExW(
        hkey,
        wide_name.as_ptr(),
        std::ptr::null_mut(),
        &mut reg_type,
        std::ptr::null_mut(),
        &mut data_size,
    );

    if status != 0 || data_size == 0 || reg_type != REG_SZ {
        return None;
    }

    let mut buffer: Vec<u16> = vec![0u16; (data_size as usize / 2) + 1];
    let status = RegQueryValueExW(
        hkey,
        wide_name.as_ptr(),
        std::ptr::null_mut(),
        &mut reg_type,
        buffer.as_mut_ptr() as *mut u8,
        &mut data_size,
    );

    if status != 0 {
        return None;
    }

    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..len]).ok()
}
