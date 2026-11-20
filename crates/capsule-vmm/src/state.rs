//! Estados y eventos de salida de la máquina virtual (VM Exits).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmStatus {
    Created,
    Running,
    Paused,
    Halted,
    Shutdown,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcpuExit {
    /// La CPU ejecutó la instrucción HLT.
    Halt,
    /// Acceso a puerto I/O de entrada (`in`).
    IoIn { port: u16, size: u8 },
    /// Acceso a puerto I/O de salida (`out`).
    IoOut { port: u16, data: u32, size: u8 },
    /// Lectura de memoria mapeada en E/S (MMIO).
    MmioRead { addr: u64, size: u8 },
    /// Escritura en memoria mapeada en E/S (MMIO).
    MmioWrite { addr: u64, data: u64, size: u8 },
    /// Apagado del guest (Triple Fault o ACPI shutdown).
    Shutdown,
    /// Salida no manejada o error del hipervisor.
    Unknown(String),
}
