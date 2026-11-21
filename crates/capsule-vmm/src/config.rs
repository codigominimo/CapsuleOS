use capsule_core::types::ByteSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmBackendKind {
    Software,
    Whpx,
}

impl Default for VmBackendKind {
    fn default() -> Self {
        VmBackendKind::Software
    }
}

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub name: String,
    pub vcpu_count: usize,
    pub memory_size: ByteSize,
    pub backend: VmBackendKind,
    /// Ruta a imagen BIOS/firmware (p.ej. SeaBIOS) para arranque por firmware (M12).
    /// Si es `None`, se usa un BIOS stub mínimo integrado para pruebas deterministas.
    pub firmware_bios_path: Option<String>,
    /// Ruta a imagen UEFI/OVMF (M12.3, condicionado). No implica soporte completo.
    pub firmware_uefi_path: Option<String>,
    /// Ruta opcional a kernel Linux (bzImage) para boot directo (M13+).
    pub linux_kernel_path: Option<String>,
    /// Ruta opcional a initramfs/initrd para boot directo Linux (M13+).
    pub linux_initrd_path: Option<String>,
    /// Línea de comandos kernel Linux (M13+).
    pub linux_cmdline: Option<String>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            name: "CapsuleGuest".to_string(),
            vcpu_count: 1,
            memory_size: ByteSize::from_mib(128),
            backend: VmBackendKind::Software,
            firmware_bios_path: None,
            firmware_uefi_path: None,
            linux_kernel_path: None,
            linux_initrd_path: None,
            linux_cmdline: None,
        }
    }
}
