//! Implementación de la Máquina Virtual (VirtualMachine).

use crate::backend::write_io_result_to_regs;
use crate::config::{VmBackendKind, VmConfig};
use crate::device::VirtualDevice;
use crate::state::{VcpuExit, VmStatus};
use crate::traits::VmmError;
use capsule_boot::{LinuxBootInfo, LinuxKernelLoader};
use capsule_core::types::GuestAddress;
use capsule_cpu::{CpuExitReason, Registers, SimpleVcpu};
use capsule_devices::{
    BochsVbe, I8042Ps2Controller, IdeController, PciBus, Pic8259, Pit8254, RtcCmos, Uart16550,
    VirtioNetPci, COM1_BASE,
};
use capsule_memory::GuestMemory;
use capsule_storage::BlockDevice;
use capsule_whpx::{WhpxPartition, WhpxRegs, WhpxRunExit};

enum ExecutionBackend {
    Software(SimpleVcpu),
    Whpx(WhpxPartition),
}

pub struct VirtualMachine {
    pub config: VmConfig,
    pub memory: GuestMemory,
    exec: ExecutionBackend,
    pub devices: Vec<Box<dyn VirtualDevice>>,
    pub status: VmStatus,
    backend_kind: VmBackendKind,
    whpx_fallback_reason: Option<String>,
}

impl VirtualMachine {
    /// Inicializa una nueva máquina virtual con la configuración dada.
    /// Registra dispositivos de plataforma básicos: COM1 (UART 16550), PIC 8259, PIT 8254,
    /// RTC/CMOS, controlador PS/2 i8042 (teclado/ratón), Bochs VBE (framebuffer, M9),
    /// un bus PCI (config mechanism #1) y una NIC virtio-net transitional con backend NAT (M8).
    /// Selecciona backend de ejecución (software o WHPX) según `config.backend`, con fallback a software.
    pub fn new(mut config: VmConfig) -> Result<Self, VmmError> {
        let mem_size = config.memory_size.as_bytes() as usize;
        let memory = GuestMemory::new(mem_size).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error asignando memoria guest: {}", e))
        })?;

        let mut fallback_reason = None;
        let (exec, backend_kind) = match config.backend {
            VmBackendKind::Software => (ExecutionBackend::Software(SimpleVcpu::new(0)), VmBackendKind::Software),
            VmBackendKind::Whpx => {
                match WhpxPartition::create_and_map(&memory) {
                    Ok(p) => (ExecutionBackend::Whpx(p), VmBackendKind::Whpx),
                    Err(e) => {
                        fallback_reason = Some(e.to_string());
                        config.backend = VmBackendKind::Software;
                        (ExecutionBackend::Software(SimpleVcpu::new(0)), VmBackendKind::Software)
                    }
                }
            }
        };

        let mut vm = Self {
            config,
            memory,
            exec,
            devices: Vec::new(),
            status: VmStatus::Created,
            backend_kind,
            whpx_fallback_reason: fallback_reason,
        };

        vm.init_default_devices();
        Ok(vm)
    }

    /// Crea una VM forzando el backend WHPX (sin fallback). Útil para validación explícita.
    pub fn new_whpx(mut config: VmConfig) -> Result<Self, VmmError> {
        config.backend = VmBackendKind::Whpx;
        let mem_size = config.memory_size.as_bytes() as usize;
        let memory = GuestMemory::new(mem_size).map_err(|e| {
            VmmError::MemoryMappingFailed(format!("Error asignando memoria guest: {}", e))
        })?;
        let p = WhpxPartition::create_and_map(&memory).map_err(|e| VmmError::InitializationFailed(e.to_string()))?;
        let mut vm = Self {
            config,
            memory,
            exec: ExecutionBackend::Whpx(p),
            devices: Vec::new(),
            status: VmStatus::Created,
            backend_kind: VmBackendKind::Whpx,
            whpx_fallback_reason: None,
        };
        vm.init_default_devices();
        Ok(vm)
    }

    fn init_default_devices(&mut self) {
        self.add_device(Box::new(Uart16550::new(COM1_BASE)));
        self.add_device(Box::new(Pic8259::new()));
        self.add_device(Box::new(Pit8254::new()));
        self.add_device(Box::new(RtcCmos::new()));
        self.add_device(Box::new(I8042Ps2Controller::new()));
        self.add_device(Box::new(BochsVbe::new()));
        let mut pci = PciBus::new();
        pci.attach_device(Box::new(VirtioNetPci::new_default()));
        self.add_device(Box::new(pci));
    }

    pub fn backend_kind(&self) -> VmBackendKind { self.backend_kind }
    pub fn whpx_fallback_reason(&self) -> Option<&str> { self.whpx_fallback_reason.as_deref() }

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
        self.set_rip(0x7C00);
        // convención: DL=0x80 (primer disco)
        let mut regs = self.get_regs();
        regs.rdx = (regs.rdx & !0xFF) | 0x80;
        self.set_regs(&regs);
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

        let mut regs = Registers::new();
        regs.rip = boot_info.entry_rip;
        regs.rsi = boot_info.boot_params_addr.raw();
        regs.rax = 0;
        regs.rbx = 0;
        regs.rcx = 0;
        regs.rdx = 0;
        regs.rdi = 0;
        regs.rbp = 0;
        self.set_regs(&regs);

        Ok(boot_info)
    }

    pub fn get_regs(&mut self) -> Registers {
        match &mut self.exec {
            ExecutionBackend::Software(cpu) => cpu.regs,
            ExecutionBackend::Whpx(p) => match p.get_regs() {
                Ok(r) => whpx_to_regs(r),
                Err(_) => Registers::new(),
            },
        }
    }

    pub fn set_regs(&mut self, regs: &Registers) {
        match &mut self.exec {
            ExecutionBackend::Software(cpu) => cpu.regs = *regs,
            ExecutionBackend::Whpx(p) => {
                let _ = p.set_regs(&regs_to_whpx(regs));
            }
        }
    }

    /// Establece el puntero de instrucción (RIP) de la vCPU.
    pub fn set_rip(&mut self, rip: u64) {
        match &mut self.exec {
            ExecutionBackend::Software(cpu) => cpu.regs.rip = rip,
            ExecutionBackend::Whpx(p) => {
                if let Ok(mut r) = p.get_regs() {
                    r.rip = rip;
                    let _ = p.set_regs(&r);
                }
            }
        }
    }

    /// Retorna el puntero de instrucción (RIP) actual.
    pub fn rip(&mut self) -> u64 {
        self.get_regs().rip
    }

    pub fn eax(&mut self) -> u32 { self.get_regs().eax() }
    pub fn rsi(&mut self) -> u64 { self.get_regs().rsi }

    fn tick_platform(&mut self) {
        // Avance aproximado de dispositivos temporizados (1 "tick" por instrucción/iteración).
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

    fn dispatch_io_out(&mut self, port: u16, data: u32, size: u8) -> bool {
        let mut handled = false;
        for dev in &mut self.devices {
            if dev.handles_port(port) {
                dev.write_io(port, data, size);
                handled = true;
                break;
            }
        }
        handled
    }

    fn dispatch_io_in(&mut self, port: u16, size: u8) -> u32 {
        for dev in &mut self.devices {
            if dev.handles_port(port) {
                return dev.read_io(port, size);
            }
        }
        0xFFFFFFFFu32
    }

    fn io_in_handled(&self, port: u16) -> bool {
        self.devices.iter().any(|d| d.handles_port(port))
    }

    /// Ejecuta un paso de la VM.
    /// - Software: tick + una instrucción.
    /// - WHPX: tick + una ejecución hasta VM-exit (puede abarcar muchas instrucciones).
    pub fn step_once(&mut self) -> Result<Option<VcpuExit>, VmmError> {
        if self.status == VmStatus::Created || self.status == VmStatus::Paused {
            self.status = VmStatus::Running;
        }
        self.tick_platform();

        match self.backend_kind {
            VmBackendKind::Software => {
                let cpu_res = match &mut self.exec {
                    ExecutionBackend::Software(cpu) => cpu.step(&mut self.memory),
                    ExecutionBackend::Whpx(_) => return Err(VmmError::Unsupported),
                };
                match cpu_res {
                    Ok(None) => Ok(None),
                    Ok(Some(CpuExitReason::Halt)) => { self.status = VmStatus::Halted; Ok(Some(VcpuExit::Halt)) }
                    Ok(Some(CpuExitReason::IoOut { port, data, size })) => {
                        let handled = self.dispatch_io_out(port, data, size);
                        self.tick_platform();
                        if handled { Ok(None) } else { Ok(Some(VcpuExit::IoOut { port, data, size })) }
                    }
                    Ok(Some(CpuExitReason::IoIn { port, size })) => {
                        let handled = self.io_in_handled(port);
                        if handled {
                            let val = self.dispatch_io_in(port, size);
                            if let ExecutionBackend::Software(cpu) = &mut self.exec {
                                write_io_result_to_regs(&mut cpu.regs, size, val);
                            }
                        }
                        self.tick_platform();
                        if handled { Ok(None) } else { Ok(Some(VcpuExit::IoIn { port, size })) }
                    }
                    Ok(Some(CpuExitReason::InvalidOpcode(op))) => {
                        let rip = self.rip();
                        self.status = VmStatus::Error;
                        Err(VmmError::VcpuError(format!("Opcode no válido ejecutado: 0x{:02X} en RIP 0x{:016X}", op, rip)))
                    }
                    Ok(Some(CpuExitReason::MemoryFault(msg))) => { self.status = VmStatus::Error; Err(VmmError::MemoryMappingFailed(msg)) }
                    Err(err) => { self.status = VmStatus::Error; Err(VmmError::VcpuError(err)) }
                }
            }
            VmBackendKind::Whpx => {
                self.run_whpx_until_exit_or_continue()
            }
        }
    }

    /// Para WHPX: ejecuta hasta una salida significativa o maneja un IO y retorna None (continuar).
    fn run_whpx_until_exit_or_continue(&mut self) -> Result<Option<VcpuExit>, VmmError> {
        loop {
            let mut whpx_exit = match &mut self.exec {
                ExecutionBackend::Whpx(p) => p.run().map_err(|e| VmmError::VcpuError(e.to_string()))?,
                _ => return Err(VmmError::Unsupported),
            };
            // Fallback: si no se pudo obtener longitud de instrucción IO desde contexto de salida, decodificar desde memoria guest.
            if let WhpxRunExit::IoPort { ref mut instruction_len, .. } = whpx_exit {
                if *instruction_len == 0 {
                    if let Ok(r) = match &mut self.exec { ExecutionBackend::Whpx(p) => p.get_regs(), _ => unreachable!() } {
                        *instruction_len = self.decode_io_instruction_len(r.rip);
                    }
                }
            }
            match whpx_exit {
                WhpxRunExit::Halted => { self.status = VmStatus::Halted; return Ok(Some(VcpuExit::Halt)); }
                WhpxRunExit::Canceled | WhpxRunExit::Interrupted => { self.tick_platform(); return Ok(None); }
                WhpxRunExit::IoPort { port, access_size, is_write, rax, instruction_len } => {
                    if is_write {
                        let data = (rax & mask_for_size(access_size)) as u32;
                        let handled = self.dispatch_io_out(port, data, access_size);
                        // advance RIP and tick
                        if instruction_len > 0 {
                            if let ExecutionBackend::Whpx(p) = &mut self.exec { let _ = p.advance_rip(instruction_len as u64); }
                        } else {
                            // Si no se pudo decodificar longitud, no avanzar RIP causaría bucle infinito.
                            // Intentar continuar sin avance solo no es seguro; reportar si no se puede avanzar.
                        }
                        self.tick_platform();
                        if handled {
                            if instruction_len == 0 {
                                self.status = VmStatus::Error;
                                return Err(VmmError::VcpuError("WHPX IO OUT manejado pero longitud de instruccion desconocida; no se puede continuar".into()));
                            }
                            return Ok(None);
                        } else {
                            return Ok(Some(VcpuExit::IoOut { port, data, size: access_size }));
                        }
                    } else {
                        let handled = self.io_in_handled(port);
                        let val = if handled { self.dispatch_io_in(port, access_size) } else { 0xFFFF_FFFFu32 };
                        if let ExecutionBackend::Whpx(p) = &mut self.exec {
                            if handled {
                                let mut rr = whpx_to_regs(p.get_regs().map_err(|e| VmmError::VcpuError(e.to_string()))?);
                                write_io_result_to_regs(&mut rr, access_size, val);
                                if instruction_len > 0 {
                                    rr.rip = rr.rip.wrapping_add(instruction_len as u64);
                                }
                                p.set_regs(&regs_to_whpx(&rr)).map_err(|e| VmmError::VcpuError(e.to_string()))?;
                            } else if instruction_len > 0 {
                                let _ = p.advance_rip(instruction_len as u64);
                            }
                        }
                        self.tick_platform();
                        if handled { return Ok(None); } else { return Ok(Some(VcpuExit::IoIn { port, size: access_size })); }
                    }
                }
                WhpxRunExit::MemoryAccess { gpa } => {
                    // No MMIO dispatch model; report as unsupported/fault-like
                    self.status = VmStatus::Error;
                    return Err(VmmError::MemoryMappingFailed(format!("WHPX memory/MMIO access no soportado en GPA 0x{gpa:X}")));
                }
                WhpxRunExit::Unsupported(s) => { self.status = VmStatus::Error; return Err(VmmError::VcpuError(format!("Salida WHPX no soportada: {s}"))); }
                WhpxRunExit::Failed(s) => { self.status = VmStatus::Error; return Err(VmmError::VcpuError(format!("Fallo WHPX: {s}"))); }
            }
        }
    }

    /// Avanza la VM hasta max_steps instrucciones/eventos o hasta una salida.
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

    /// Devuelve información del framebuffer VBE si está habilitado con LFB.
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
    /// adecuado para DIB top-down de 32bpp. Devuelve (w,h) si hay framebuffer válido.
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

    /// Encola scancodes PS/2 Set 1 (teclado) en el controlador i8042, si está presente.
    pub fn inject_ps2_scancodes(&mut self, scancodes: &[u8]) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_scancodes(scancodes);
                break;
            }
        }
    }

    /// Encola evento de ratón PS/2 (movimiento relativo, botones, rueda) en el controlador i8042.
    pub fn inject_ps2_mouse_event(&mut self, dx: i32, dy: i32, buttons: u8, wheel: i32) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_mouse_event(dx, dy, buttons, wheel);
                break;
            }
        }
    }

    /// Encola un paquete PS/2 de ratón sin movimiento (p.ej. cambio de botones).
    pub fn inject_ps2_mouse_button_packet(&mut self) {
        for dev in &mut self.devices {
            if let Some(ps2) = dev.as_any_mut().downcast_mut::<I8042Ps2Controller>() {
                ps2.enqueue_mouse_button_packet();
                break;
            }
        }
    }

    /// Ejecuta el bucle principal de la VM hasta alcanzar una condición de salida.
    pub fn run(&mut self) -> Result<VcpuExit, VmmError> {
        self.status = VmStatus::Running;
        loop {
            if let Some(exit) = self.step_once()? {
                // step_once may return None for handled IO; Some for terminal/unhandled
                if matches!(exit, VcpuExit::Halt) || matches!(exit, VcpuExit::IoIn{..}|VcpuExit::IoOut{..}|VcpuExit::Shutdown) {
                    return Ok(exit);
                }
                return Ok(exit);
            }
            // continue
        }
    }
}

fn mask_for_size(size: u8) -> u64 {
    match size { 1 => 0xFF, 2 => 0xFFFF, 4 => 0xFFFF_FFFF, _ => u64::MAX }
}

impl VirtualMachine {
    /// Decodificación mínima de longitud para instrucciones IN/OUT (puerto inmediato o DX), con prefijos 0x66/0x67.
    /// Usado como fallback cuando WHPX no proporciona longitud de instrucción en el contexto de salida.
    fn decode_io_instruction_len(&self, rip: u64) -> u8 {
        let mut off = 0u64;
        let mut len: u8 = 0;
        for _ in 0..4 {
            let b = match self.memory.read_u8(GuestAddress::new(rip.wrapping_add(off))) { Ok(v) => v, Err(_) => return 0 };
            match b {
                0x66 | 0x67 | 0xF2 | 0xF3 | 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 => {
                    len = len.saturating_add(1);
                    off += 1;
                    continue;
                }
                0x40..=0x4F => { // REX
                    len = len.saturating_add(1);
                    off += 1;
                    continue;
                }
                _ => break,
            }
        }
        let op = match self.memory.read_u8(GuestAddress::new(rip.wrapping_add(off))) { Ok(v) => v, Err(_) => return 0 };
        let op_len = match op {
            0xE4 | 0xE5 | 0xE6 | 0xE7 => 2, // op imm8
            0xEC | 0xED | 0xEE | 0xEF => 1, // op (DX)
            _ => 0,
        };
        if op_len == 0 { 0 } else { len.saturating_add(op_len) }
    }
}

fn regs_to_whpx(r: &Registers) -> WhpxRegs {
    WhpxRegs {
        rax: r.rax, rbx: r.rbx, rcx: r.rcx, rdx: r.rdx,
        rsi: r.rsi, rdi: r.rdi, rbp: r.rbp, rsp: r.rsp,
        r8: r.r8, r9: r.r9, r10: r.r10, r11: r.r11,
        r12: r.r12, r13: r.r13, r14: r.r14, r15: r.r15,
        rip: r.rip, rflags: r.rflags,
    }
}

fn whpx_to_regs(r: WhpxRegs) -> Registers {
    Registers {
        rax: r.rax, rbx: r.rbx, rcx: r.rcx, rdx: r.rdx,
        rsi: r.rsi, rdi: r.rdi, rbp: r.rbp, rsp: r.rsp,
        r8: r.r8, r9: r.r9, r10: r.r10, r11: r.r11,
        r12: r.r12, r13: r.r13, r14: r.r14, r15: r.r15,
        rip: r.rip, rflags: r.rflags,
    }
}
