//! Implementación de la Máquina Virtual (VirtualMachine).

use crate::config::VmConfig;
use crate::device::VirtualDevice;
use crate::state::{VcpuExit, VmStatus};
use crate::traits::VmmError;
use capsule_boot::{LinuxBootInfo, LinuxKernelLoader};
use capsule_core::types::GuestAddress;
use capsule_cpu::{CpuExitReason, SimpleVcpu};
use capsule_devices::{
    BochsVbe, I8042Ps2Controller, IdeController, PciBus, Pic8259, Pit8254, RtcCmos, Uart16550,
    VirtioNetPci, COM1_BASE,
};
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
    /// Registra dispositivos de plataforma basicos: COM1 (UART 16550), PIC 8259, PIT 8254,
    /// RTC/CMOS, controlador PS/2 i8042 (teclado/raton), Bochs VBE (framebuffer, M9),
    /// un bus PCI (config mechanism #1) y una NIC virtio-net transitional con backend NAT (M8).
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

        vm.add_device(Box::new(Uart16550::new(COM1_BASE)));
        vm.add_device(Box::new(Pic8259::new()));
        vm.add_device(Box::new(Pit8254::new()));
        vm.add_device(Box::new(RtcCmos::new()));
        vm.add_device(Box::new(I8042Ps2Controller::new()));
        vm.add_device(Box::new(BochsVbe::new()));

        let mut pci = PciBus::new();
        pci.attach_device(Box::new(VirtioNetPci::new_default()));
        vm.add_device(Box::new(pci));

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

    fn tick_platform(&mut self) {
        // Avance aproximado de dispositivos temporizados (1 "tick" por instrucción).
        // PIT canal 0 genera IRQ0 al expirar según su modo/programación.
        let mut pit_irq0 = false;
        for dev in &mut self.devices {
            if let Some(pit) = dev.as_any_mut().downcast_mut::<Pit8254>() {
                pit_irq0 = pit.tick(1);
                break;
            }
        }
        // RTC: refresco ligero de hora en cada tick (modelo simplificado).
        for dev in &mut self.devices {
            if let Some(rtc) = dev.as_any_mut().downcast_mut::<RtcCmos>() {
                rtc.tick();
                break;
            }
        }

        // Sondeo de dispositivos PCI con acceso a memoria guest (p.ej. virtio-net virtqueues + NAT).
        {
            let memory = &mut self.memory;
            for dev in &mut self.devices {
                if let Some(pci) = dev.as_any_mut().downcast_mut::<PciBus>() {
                    pci.poll_devices(memory);
                    break;
                }
            }
        }

        // Cableado de IRQs de plataforma hacia PIC 8259.
        // - IRQ0 : PIT (pulso en flanco aproximado por tick)
        // - IRQ1 : Teclado PS/2 i8042 (nivel mientras haya scancodes pendientes)
        // - IRQ11: PCI INTx agregada (virtio-net, nivel mientras ISR pendiente)
        // - IRQ12: Ratón PS/2 i8042 (nivel mientras haya paquetes pendientes)
        // - IRQ14: IDE primario (nivel mientras irq_pending && !nIEN)
        let mut ide_irq14 = false;
        let mut pci_irq11 = false;
        let mut kbd_irq1 = false;
        let mut mouse_irq12 = false;
        for dev in &mut self.devices {
            if let Some(ide) = dev.as_any_mut().downcast_mut::<IdeController>() {
                ide_irq14 = ide.irq_pending();
            }
            if let Some(pci) = dev.as_any_mut().downcast_mut::<PciBus>() {
                pci_irq11 = pci.any_irq_level();
            }
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                kbd_irq1 = ps2.keyboard_irq_pending();
                mouse_irq12 = ps2.mouse_irq_pending();
            }
        }

        for dev in &mut self.devices {
            if let Some(pic) = dev.as_any_mut().downcast_mut::<Pic8259>() {
                if pit_irq0 {
                    pic.raise_irq(0);
                }
                pic.set_irq_level(1, kbd_irq1);
                pic.set_irq_level(11, pci_irq11);
                pic.set_irq_level(12, mouse_irq12);
                pic.set_irq_level(14, ide_irq14);
                break;
            }
        }
    }

    /// Ejecuta un paso de la VM (tick de plataforma + una instruccion de vCPU).
    /// Devuelve Ok(None) si debe continuar, Ok(Some(exit)) si hay salida,
    /// o error en fallo. Util para bucles de UI/tiempo real que intercalan renderizado.
    pub fn step_once(&mut self) -> Result<Option<VcpuExit>, VmmError> {
        if self.status == VmStatus::Created || self.status == VmStatus::Paused {
            self.status = VmStatus::Running;
        }
        self.tick_platform();
        match self.vcpu.step(&mut self.memory) {
            Ok(None) => Ok(None),
            Ok(Some(CpuExitReason::Halt)) => {
                self.status = VmStatus::Halted;
                Ok(Some(VcpuExit::Halt))
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
                self.tick_platform();
                if handled {
                    Ok(None)
                } else {
                    Ok(Some(VcpuExit::IoOut { port, data, size }))
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
                self.tick_platform();
                if handled {
                    Ok(None)
                } else {
                    Ok(Some(VcpuExit::IoIn { port, size }))
                }
            }
            Ok(Some(CpuExitReason::InvalidOpcode(op))) => {
                self.status = VmStatus::Error;
                Err(VmmError::VcpuError(format!(
                    "Opcode no valido ejecutado: 0x{:02X} en RIP 0x{:016X}",
                    op, self.vcpu.regs.rip
                )))
            }
            Ok(Some(CpuExitReason::MemoryFault(msg))) => {
                self.status = VmStatus::Error;
                Err(VmmError::MemoryMappingFailed(msg))
            }
            Err(err) => {
                self.status = VmStatus::Error;
                Err(VmmError::VcpuError(err))
            }
        }
    }

    /// Avanza la VM hasta max_steps instrucciones o hasta una salida.
    pub fn run_steps(&mut self, max_steps: usize) -> Result<Option<VcpuExit>, VmmError> {
        for _ in 0..max_steps {
            if let Some(exit) = self.step_once()? {
                return Ok(Some(exit));
            }
            if self.status != VmStatus::Running {
                break;
            }
        }
        Ok(None)
    }

    /// Devuelve informacion del framebuffer VBE si esta habilitado con LFB.
    /// (base_gpa, width, height, bpp, stride_bytes)
    pub fn vbe_framebuffer_info(&self) -> Option<(u64, u32, u32, u32, usize)> {
        for dev in &self.devices {
            if let Some(vbe) = dev.as_any().downcast_ref::<BochsVbe>() {
                if vbe.is_enabled() && vbe.is_lfb_enabled() {
                    return Some((
                        vbe.lfb_base(),
                        vbe.width(),
                        vbe.height(),
                        vbe.bpp(),
                        vbe.stride_bytes(),
                    ));
                }
            }
        }
        None
    }

    /// Copia el framebuffer lineal guest a un buffer BGRA32 (0x00RRGGBB little-endian en memoria => B,G,R,X)
    /// adecuado para DIB top-down de 32bpp. Devuelve (w,h) si hay framebuffer valido.
    pub fn copy_framebuffer_bgra32(&self, out: &mut [u32]) -> Option<(u32, u32)> {
        let (base, w, h, bpp, stride) = self.vbe_framebuffer_info()?;
        if w == 0 || h == 0 { return None; }
        let needed = (w as usize).saturating_mul(h as usize);
        if out.len() < needed { return None; }
        let mem_size = self.memory.size();
        let base_usize = base as usize;
        for y in 0..(h as usize) {
            let row_off = base_usize.saturating_add(y.saturating_mul(stride));
            for x in 0..(w as usize) {
                let idx = y * (w as usize) + x;
                let px = match bpp {
                    32 => {
                        let off = row_off + x * 4;
                        if off + 4 > mem_size { 0 } else {
                            let mut b = [0u8; 4];
                            if self.memory.read_bytes(GuestAddress::new((off) as u64), &mut b).is_ok() {
                                u32::from_le_bytes(b)
                            } else { 0 }
                        }
                    }
                    24 => {
                        let off = row_off + x * 3;
                        if off + 3 > mem_size { 0 } else {
                            let mut b = [0u8; 3];
                            if self.memory.read_bytes(GuestAddress::new((off) as u64), &mut b).is_ok() {
                                (b[0] as u32) | ((b[1] as u32) << 8) | ((b[2] as u32) << 16) | 0xFF00_0000
                            } else { 0 }
                        }
                    }
                    16 | 15 => {
                        let off = row_off + x * 2;
                        if off + 2 > mem_size { 0 } else {
                            let mut b = [0u8; 2];
                            if self.memory.read_bytes(GuestAddress::new((off) as u64), &mut b).is_ok() {
                                let v = u16::from_le_bytes(b);
                                // RGB565
                                let r5 = ((v >> 11) & 0x1F) as u32;
                                let g6 = ((v >> 5) & 0x3F) as u32;
                                let b5 = (v & 0x1F) as u32;
                                let r = (r5 * 255 / 31) & 0xFF;
                                let g = (g6 * 255 / 63) & 0xFF;
                                let bb = (b5 * 255 / 31) & 0xFF;
                                (bb) | (g << 8) | (r << 16) | 0xFF00_0000
                            } else { 0 }
                        }
                    }
                    _ => 0,
                };
                out[idx] = px;
            }
        }
        Some((w, h))
    }

    /// Encola scancodes PS/2 Set 1 (teclado) en el controlador i8042, si esta presente.
    pub fn inject_ps2_scancodes(&mut self, scancodes: &[u8]) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_scancodes(scancodes);
                break;
            }
        }
    }

    /// Encola evento de raton PS/2 (movimiento relativo, botones, rueda) en el controlador i8042.
    /// `buttons`: bit0=izq, bit1=der, bit2=medio.
    pub fn inject_ps2_mouse_event(&mut self, dx: i32, dy: i32, buttons: u8, wheel: i32) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_mouse_event(dx, dy, buttons, wheel);
                break;
            }
        }
    }

    /// Encola un paquete PS/2 de raton sin movimiento (p.ej. cambio de botones).
    pub fn inject_ps2_mouse_button_packet(&mut self) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_mouse_button_packet();
                break;
            }
        }
    }

    /// Ejecuta el bucle principal de la vCPU hasta alcanzar una condición de salida (HALT, Error, etc.).
    /// El bucle avanza dispositivos de plataforma (PIT/RTC), sondea dispositivos PCI (virtqueues/NAT)
    /// con acceso a memoria guest y actualiza líneas IRQ básicas (PIC, IDE IRQ14, PCI INTx/IRQ11).
    /// La inyección de interrupciones en la CPU software no está implementada aún; los guests deben
    /// usar sondeo (polling) o esperar soporte futuro de interrupciones/IDT.
    pub fn run(&mut self) -> Result<VcpuExit, VmmError> {
        self.status = VmStatus::Running;

        loop {
            self.tick_platform();
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

                    // La E/S puede cambiar estado de dispositivos (p.ej. PIC/IDE/PCI/PS2); refrescar.
                    self.tick_platform();

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

                    // Lecturas de estado (IDE/PIC/RTC/PCI) pueden reconocer/limpiar condiciones.
                    self.tick_platform();

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
