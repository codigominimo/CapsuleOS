//! Implementación de la Máquina Virtual (VirtualMachine).

use crate::config::VmConfig;
use crate::device::VirtualDevice;
use crate::state::{VcpuExit, VmStatus};
use crate::traits::VmmError;
use capsule_core::types::GuestAddress;
use capsule_cpu::{CpuExitReason, SimpleVcpu};
use capsule_memory::GuestMemory;

pub struct VirtualMachine {
    pub config: VmConfig,
    pub memory: GuestMemory,
    pub vcpu: SimpleVcpu,
    pub devices: Vec<Box<dyn VirtualDevice>>,
    pub status: VmStatus,
}

impl VirtualMachine {
    /// Inicializa una nueva máquina virtual con la configuración dada.
    pub fn new(config: VmConfig) -> Result<Self, VmmError> {
        let mem_size = config.memory_size.as_bytes() as usize;
        let memory = GuestMemory::new(mem_size).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error asignando memoria guest: {}", e))
        })?;

        let vcpu = SimpleVcpu::new(0);

        Ok(Self {
            config,
            memory,
            vcpu,
            devices: Vec::new(),
            status: VmStatus::Created,
        })
    }

    /// Registra un dispositivo virtual en la máquina.
    pub fn add_device(&mut self, device: Box<dyn VirtualDevice>) {
        self.devices.push(device);
    }

    /// Carga un bloque de código máquina ejecutable en la memoria física del guest.
    pub fn load_code(&mut self, gpa: GuestAddress, code: &[u8]) -> Result<(), VmmError> {
        self.memory.write_bytes(gpa, code).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error cargando código en {}: {}", gpa, e))
        })?;
        Ok(())
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
                    // Instrucción normal ejecutada, continuar ciclo
                    continue;
                }
                Ok(Some(CpuExitReason::Halt)) => {
                    self.status = VmStatus::Halted;
                    return Ok(VcpuExit::Halt);
                }
                Ok(Some(CpuExitReason::IoOut { port, data, size })) => {
                    // Verificar si un dispositivo virtual maneja este puerto
                    let mut handled = false;
                    for dev in &mut self.devices {
                        if dev.handles_port(port) {
                            dev.write_io(port, data, size);
                            handled = true;
                            break;
                        }
                    }

                    if !handled {
                        // Salida hacia el host
                        return Ok(VcpuExit::IoOut { port, data, size });
                    }
                }
                Ok(Some(CpuExitReason::IoIn { port, size })) => {
                    let mut handled = false;
                    for dev in &mut self.devices {
                        if dev.handles_port(port) {
                            let val = dev.read_io(port, size);
                            match size {
                                1 => self.vcpu.regs.rax = (self.vcpu.regs.rax & !0xFF) | (val as u64 & 0xFF),
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
