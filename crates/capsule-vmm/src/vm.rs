//! Implementación de la Máquina Virtual (VirtualMachine).

use crate::config::VmConfig;
use crate::device::VirtualDevice;
use crate::state::{VcpuExit, VmStatus};
use crate::traits::VmmError;
use capsule_boot::{LinuxBootInfo, LinuxKernelLoader};
use capsule_core::types::GuestAddress;
use capsule_cpu::{CpuExitReason, SimpleVcpu};
use capsule_devices::{IdeController, Uart16550, COM1_BASE};
use capsule_memory::GuestMemory;
use capsule_storage::BlockDevice;

pub struct VirtualMachine {
    pub config: VmConfig,
    pub memory: GuestMemory,
    pub vcpu: SimpleVcpu,
    pub devices: Vec<Box<dyn VirtualDevice>>,
    pub status: VmStatus,
}

impl VirtualMachine {
    /// Inicializa una nueva máquina virtual con la configuración dada.
    /// Registra automáticamente el puerto serie COM1 (UART 16550) en el puerto I/O 0x3F8.
    pub fn new(config: VmConfig) -> Result<Self, VmmError> {
        let mem_size = config.memory_size.as_bytes() as usize;
        let memory = GuestMemory::new(mem_size).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error asignando memoria guest: {}", e))
        })?;

        let vcpu = SimpleVcpu::new(0);

        let mut vm = Self {
            config,
            memory,
            vcpu,
            devices: Vec::new(),
            status: VmStatus::Created,
        };

        // Dispositivo estándar: COM1 UART 16550 en 0x3F8
        vm.add_device(Box::new(Uart16550::new(COM1_BASE)));

        Ok(vm)
    }

    /// Registra un dispositivo virtual en la máquina.
    pub fn add_device(&mut self, device: Box<dyn VirtualDevice>) {
        self.devices.push(device);
    }

    /// Adjunta un disco al canal IDE primario como unidad maestra (master).
    /// Registra el controlador ATA/IDE PIO (0x1F0-0x1F7, 0x3F6) si aún no existe.
    pub fn attach_primary_master_disk(
        &mut self,
        disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        let ide_port = 0x1F0u16;
        if self.devices.iter().any(|d| d.handles_port(ide_port)) {
            return Err(VmmError::InitializationFailed(
                "El controlador IDE primario ya está registrado".to_string(),
            ));
        }
        let mut ide = IdeController::new_primary();
        ide.attach_master(disk);
        self.devices.push(Box::new(ide));
        Ok(())
    }

    /// Carga el sector de arranque (LBA 0, 512 bytes) desde un disco en 0x7C00,
    /// configura RIP=0x7C00 y adjunta el disco al IDE primario maestro.
    /// Permite un flujo mínimo de "boot desde disco" usando un stub de arranque plano.
    pub fn boot_from_primary_master_disk(
        &mut self,
        mut disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        if disk.sector_size() != 512 {
            return Err(VmmError::InitializationFailed(format!(
                "Solo se soporta boot desde disco con sectores de 512 bytes (recibido: {})",
                disk.sector_size()
            )));
        }
        let mut boot_sector = vec![0u8; 512];
        disk.read_sectors(0, 1, &mut boot_sector).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error leyendo sector de arranque LBA 0: {}", e))
        })?;
        self.memory
            .write_bytes(GuestAddress::new(0x7C00), &boot_sector)
            .map_err(|e| {
                VmmError::MemoryMappingFailed(format!(
                    "Error cargando sector de arranque en 0x7C00: {}",
                    e
                ))
            })?;
        self.vcpu.regs.rip = 0x7C00;
        self.vcpu.regs.rdx = 0x0080; // convención: DL=0x80 (primer disco)
        self.attach_primary_master_disk(disk)?;
        Ok(())
    }

    /// Carga un bloque de código máquina ejecutable en la memoria física del guest.
    pub fn load_code(&mut self, gpa: GuestAddress, code: &[u8]) -> Result<(), VmmError> {
        self.memory.write_bytes(gpa, code).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error cargando código en {}: {}", gpa, e))
        })?;
        Ok(())
    }

    /// Carga un kernel Linux real o sintético según el protocolo de arranque x86 directo (bzImage direct boot).
    /// Configura las tablas de páginas PML4, la Zero Page (boot_params), la línea de comandos,
    /// el initramfs y los registros de arranque (`RSI` -> `boot_params`, `RIP` -> kernel entry).
    pub fn load_linux(
        &mut self,
        kernel_bytes: &[u8],
        cmdline: &str,
        initramfs_bytes: Option<&[u8]>,
    ) -> Result<LinuxBootInfo, VmmError> {
        let boot_info = LinuxKernelLoader::load(
            &mut self.memory,
            kernel_bytes,
            cmdline,
            initramfs_bytes,
        )
        .map_err(|e| VmmError::InitializationFailed(e))?;

        // Configuración de registros para Linux Boot Protocol:
        self.vcpu.regs.rip = boot_info.entry_rip;
        self.vcpu.regs.rsi = boot_info.boot_params_addr.raw();
        self.vcpu.regs.rax = 0;
        self.vcpu.regs.rbx = 0;
        self.vcpu.regs.rcx = 0;
        self.vcpu.regs.rdx = 0;
        self.vcpu.regs.rdi = 0;
        self.vcpu.regs.rbp = 0;

        Ok(boot_info)
    }

    /// Establece el puntero de instrucción (RIP) de la vCPU.
    pub fn set_rip(&mut self, rip: u64) {
        self.vcpu.regs.rip = rip;
    }

    /// Retorna el puntero de instrucción (RIP) actual.
    pub fn rip(&self) -> u64 {
        self.vcpu.regs.rip
    }

    /// Ejecuta el bucle principal de la vCPU hasta alcanzar una condición de salida (HALT, Error, etc.).
    pub fn run(&mut self) -> Result<VcpuExit, VmmError> {
        self.status = VmStatus::Running;

        loop {
            match self.vcpu.step(&mut self.memory) {
                Ok(None) => {
                    continue;
                }
                Ok(Some(CpuExitReason::Halt)) => {
                    self.status = VmStatus::Halted;
                    return Ok(VcpuExit::Halt);
                }
                Ok(Some(CpuExitReason::IoOut { port, data, size })) => {
                    let mut handled = false;
                    for dev in &mut self.devices {
                        if dev.handles_port(port) {
                            dev.write_io(port, data, size);
                            handled = true;
                            break;
                        }
                    }

                    if !handled {
                        return Ok(VcpuExit::IoOut { port, data, size });
                    }
                }
                Ok(Some(CpuExitReason::IoIn { port, size })) => {
                    let mut handled = false;
                    for dev in &mut self.devices {
                        if dev.handles_port(port) {
                            let val = dev.read_io(port, size);
                            match size {
                                1 => {
                                    self.vcpu.regs.rax =
                                        (self.vcpu.regs.rax & !0xFF) | (val as u64 & 0xFF)
                                }
                                2 => {
                                    self.vcpu.regs.rax =
                                        (self.vcpu.regs.rax & !0xFFFF) | (val as u64 & 0xFFFF)
                                }
                                4 => self.vcpu.regs.set_eax(val),
                                _ => {}
                            }
                            handled = true;
                            break;
                        }
                    }

                    if !handled {
                        return Ok(VcpuExit::IoIn { port, size });
                    }
                }
                Ok(Some(CpuExitReason::InvalidOpcode(op))) => {
                    self.status = VmStatus::Error;
                    return Err(VmmError::VcpuError(format!(
                        "Opcode no válido ejecutado: 0x{:02X} en RIP 0x{:016X}",
                        op, self.vcpu.regs.rip
                    )));
                }
                Ok(Some(CpuExitReason::MemoryFault(msg))) => {
                    self.status = VmStatus::Error;
                    return Err(VmmError::MemoryMappingFailed(msg));
                }
                Err(err) => {
                    self.status = VmStatus::Error;
                    return Err(VmmError::VcpuError(err));
                }
            }
        }
    }
}
