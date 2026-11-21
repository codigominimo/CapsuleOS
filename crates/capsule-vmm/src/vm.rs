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
use capsule_memory::{GuestMemory, MemoryBus};
use capsule_storage::BlockDevice;
use capsule_whpx::{WhpxPartition, WhpxRegs, WhpxRunExit};

enum ExecutionBackend {
    Software(SimpleVcpu),
    Whpx(WhpxPartition),
}

struct VmBus<'a> {
    mem: &'a mut GuestMemory,
    devices: &'a mut [Box<dyn VirtualDevice>],
}

impl<'a> MemoryBus for VmBus<'a> {
    fn read_u8(&mut self, addr: GuestAddress) -> Result<u8, String> {
        let a = addr.raw();
        for dev in self.devices.iter_mut() {
            if dev.handles_mmio(a) { return Ok(dev.read_mmio(a, 1) as u8); }
        }
        self.mem.read_u8(addr).map_err(|e| e.to_string())
    }
    fn write_u8(&mut self, addr: GuestAddress, val: u8) -> Result<(), String> {
        let a = addr.raw();
        for dev in self.devices.iter_mut() {
            if dev.handles_mmio(a) { dev.write_mmio(a, val as u64, 1); return Ok(()); }
        }
        self.mem.write_u8(addr, val).map_err(|e| e.to_string())
    }
    fn read_bytes(&mut self, addr: GuestAddress, buf: &mut [u8]) -> Result<(), String> {
        let a = addr.raw();
        for dev in self.devices.iter_mut() {
            if dev.handles_mmio(a) {
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = dev.read_mmio(a + i as u64, 1) as u8;
                }
                return Ok(());
            }
        }
        self.mem.read_bytes(addr, buf).map_err(|e| e.to_string())
    }
    fn write_bytes(&mut self, addr: GuestAddress, data: &[u8]) -> Result<(), String> {
        let a = addr.raw();
        for dev in self.devices.iter_mut() {
            if dev.handles_mmio(a) {
                for (i, b) in data.iter().enumerate() {
                    dev.write_mmio(a + i as u64, *b as u64, 1);
                }
                return Ok(());
            }
        }
        self.mem.write_bytes(addr, data).map_err(|e| e.to_string())
    }
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

    /// Adjunta un disco ATA al canal IDE primario como unidad maestra (master).
    /// Registra el controlador ATA/IDE PIO (0x1F0-0x1F7, 0x3F6) si aún no existe.
    pub fn attach_primary_master_disk(
        &mut self,
        disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        self.attach_primary_master_disk_with_kind(disk, false)
    }

    /// Adjunta un disco ATAPI/CD-ROM al canal IDE primario como unidad maestra (M12.2, experimental).
    pub fn attach_primary_master_atapi_disk(
        &mut self,
        disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        self.attach_primary_master_disk_with_kind(disk, true)
    }

    fn attach_primary_master_disk_with_kind(
        &mut self,
        disk: Box<dyn BlockDevice>,
        atapi: bool,
    ) -> Result<(), VmmError> {
        let ide_port = 0x1F0u16;
        if self.devices.iter().any(|d| d.handles_port(ide_port)) {
            return Err(VmmError::InitializationFailed(
                "El controlador IDE primario ya está registrado".to_string(),
            ));
        }
        let mut ide = IdeController::new_primary();
        if atapi { ide.attach_master_atapi(disk); } else { ide.attach_master(disk); }
        self.devices.push(Box::new(ide));
        Ok(())
    }

    /// Adjunta un disco ATAPI/CD-ROM al canal IDE secundario como unidad maestra (M12.2).
    /// Registra el controlador (0x170-0x177, 0x376) si aún no existe.
    pub fn attach_secondary_master_atapi_disk(
        &mut self,
        disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        let ide_port = 0x170u16;
        if self.devices.iter().any(|d| d.handles_port(ide_port)) {
            return Err(VmmError::InitializationFailed(
                "El controlador IDE secundario ya está registrado".to_string(),
            ));
        }
        let mut ide = IdeController::new_secondary();
        ide.attach_master_atapi(disk);
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

    /// Mapea una imagen de firmware BIOS en ROM guest y retorna el RIP de entrada/reset.
    /// - Imágenes <= 64 KiB: se mapean en la ventana BIOS clásica 0xF0000-0xFFFFF, entrada 0xFFFF0.
    /// - Imágenes mayores (p.ej. SeaBIOS como imagen plana best-effort): se mapean alineadas al
    ///   final del primer MiB (base = 1 MiB - size, mín. 0xE0000). Entrada 0xFFFF0 si está dentro
    ///   del rango mapeado; en caso contrario, inicio de la imagen (experimental).
    pub fn map_bios_rom(&mut self, data: &[u8]) -> Result<u64, VmmError> {
        self.map_firmware_rom(data, 0xE_0000)
    }

    /// Mapeo de firmware ROM genérico (M12.3: usado por ruta OVMF/UEFI condicionada).
    /// No implica soporte UEFI completo; solo mapea bytes ROM y calcula un entry determinista.
    pub fn map_firmware_rom(&mut self, data: &[u8], min_base: u64) -> Result<u64, VmmError> {
        if data.is_empty() {
            return Err(VmmError::InitializationFailed(
                "Imagen BIOS vacía".to_string(),
            ));
        }
        let (map_base, entry_rip) = if data.len() <= 0x1_0000 {
            (0xF_0000u64, 0xF_FFF0u64)
        } else {
            let size = data.len() as u64;
            let base = 0x0010_0000u64.saturating_sub(size).max(min_base.max(0xE_0000));
            (base, 0xF_FFF0u64)
        };
        let end = map_base + (data.len() as u64);
        if end > self.memory.size() as u64 {
            return Err(VmmError::MemoryMappingFailed(format!(
                "Imagen BIOS ({} bytes en 0x{:X}) excede la memoria guest ({} bytes)",
                data.len(),
                map_base,
                self.memory.size()
            )));
        }
        self.memory
            .map_rom(GuestAddress::new(map_base), data)
            .map_err(|e| VmmError::MemoryMappingFailed(format!("Error mapeando firmware ROM: {e}")))?;
        let entry = if entry_rip >= map_base && entry_rip < end {
            entry_rip
        } else {
            map_base
        };
        Ok(entry)
    }

    /// Retorna un BIOS stub integrado determinista (M12.1, 64 KiB) que imprime un banner en COM1,
    /// lee LBA0 del disco IDE primario maestro a 0x7C00 vía PIO y transfiere control al boot sector.
    /// Diseñado para el backend software actual (modelo plano, sin interrupciones BIOS reales).
    pub fn builtin_bios_stub() -> Vec<u8> {
        build_builtin_bios_stub()
    }

    /// Arranque por firmware BIOS desde disco IDE primario maestro (M12.1, backend software).
    /// Adjunta el disco, mapea BIOS (externa si se provee, o stub integrado), y establece estado de
    /// reset mínimo (RIP=vector de reset, RSP temporal, RFLAGS=0x2, DL=0x80).
    pub fn boot_firmware_from_primary_master_disk(
        &mut self,
        disk: Box<dyn BlockDevice>,
        bios_image: Option<&[u8]>,
    ) -> Result<(), VmmError> {
        if disk.sector_size() != 512 {
            return Err(VmmError::InitializationFailed(format!(
                "Solo se soporta boot firmware desde disco con sectores de 512 bytes (recibido: {})",
                disk.sector_size()
            )));
        }
        self.attach_primary_master_disk(disk)?;
        let owned;
        let bios = if let Some(img) = bios_image {
            img
        } else {
            owned = Self::builtin_bios_stub();
            &owned
        };
        let entry = self.map_bios_rom(bios)?;
 
        let mut regs = Registers::new();
        regs.rip = entry;
        regs.rsp = 0x0007_FFF0;
        regs.rdx = 0x80; // DL = primer disco duro (convención BIOS)
        regs.rflags = 0x2;
        self.set_regs(&regs);
        Ok(())
    }

    /// Boot mínimo desde CD-ROM/ISO (M12.2, scope acotado).
    /// - Adjunta el disco como ATAPI en IDE secundario maestro (0x170).
    /// - Intenta El Torito: lee PVD (LBA 16), Boot Record Volume (LBA 17, "EL TORITO SPECIFICATION"),
    ///   catalogo de arranque, entrada inicial default x86, y carga la imagen de boot en 0x7C00.
    /// - Fallback: si no hay El Torito válido, carga LBA 0 (2048 bytes) en 0x7C00.
    /// - Configura RIP=0x7C00, DL=0xE0 (CD-ROM) y estado mínimo.
    pub fn boot_from_cdrom_secondary_master(
        &mut self,
        mut disk: Box<dyn BlockDevice>,
    ) -> Result<(), VmmError> {
        let ss = disk.sector_size();
        if ss != 2048 {
            return Err(VmmError::InitializationFailed(format!(
                "M12.2 CD-ROM requiere sectores de 2048 bytes (recibido: {ss})"
            )));
        }
        let boot_img = read_cdrom_boot_image(&mut *disk)?;
        self.memory
            .write_bytes(GuestAddress::new(0x7C00), &boot_img)
            .map_err(|e| VmmError::MemoryMappingFailed(format!("Error cargando boot CD-ROM en 0x7C00: {e}")))?;
        self.attach_secondary_master_atapi_disk(disk)?;
        self.set_rip(0x7C00);
        let mut regs = self.get_regs();
        regs.rdx = (regs.rdx & !0xFF) | 0xE0; // DL=CD
        regs.rflags = 0x2;
        self.set_regs(&regs);
        Ok(())
    }

    /// M12.3 (condicionado): mapea imagen UEFI/OVMF en ROM y configura reset mínimo.
    /// No declara soporte OVMF completo; útil para validación de plumbing/mapa/reset y
    /// pruebas best-effort controladas. La ejecución real de OVMF depende de gaps conocidos
    /// (MMIO, IRQ/APIC, CPU x86_64, timers/dispositivos firmware).
    pub fn boot_firmware_uefi_image(&mut self, image: &[u8]) -> Result<u64, VmmError> {
        let entry = self.map_firmware_rom(image, 0xE_0000)?;
        let mut regs = Registers::new();
        regs.rip = entry;
        regs.rsp = 0x0007_FFF0;
        regs.rflags = 0x2;
        self.set_regs(&regs);
        Ok(entry)
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
        // - IRQ14: IDE primario, IRQ15: IDE secundario (nivel mientras irq_pending && !nIEN)
        let mut ide_irq14 = false;
        let mut ide_irq15 = false;
        let mut pci_irq11 = false;
        let mut kbd_irq1 = false;
        let mut mouse_irq12 = false;
        for dev in &mut self.devices {
            if let Some(ide) = dev.as_any_mut().downcast_mut::<IdeController>() {
                if ide.io_base == 0x1F0 {
                    ide_irq14 = ide.irq_pending();
                } else if ide.io_base == 0x170 {
                    ide_irq15 = ide.irq_pending();
                }
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
                pic.set_irq_level(15, ide_irq15);
                break;
            }
        }
    }

    /// Decodificacion minima de longitud para instrucciones IN/OUT.
    fn decode_io_instruction_len(&self, rip: u64) -> u8 {
        let mut off = 0u64; let mut len: u8 = 0;
        for _ in 0..4 {
            let b = match self.memory.read_u8(GuestAddress::new(rip.wrapping_add(off))) { Ok(v) => v, Err(_) => return 0 };
            match b {
                0x66|0x67|0xF2|0xF3|0x26|0x2E|0x36|0x3E|0x64|0x65 => { len=len.saturating_add(1); off+=1; continue; }
                0x40..=0x4F => { len=len.saturating_add(1); off+=1; continue; }
                _ => break,
            }
        }
        let op = match self.memory.read_u8(GuestAddress::new(rip.wrapping_add(off))) { Ok(v) => v, Err(_) => return 0 };
        let op_len = match op { 0xE4|0xE5|0xE6|0xE7 => 2, 0xEC|0xED|0xEE|0xEF => 1, _ => 0 };
        if op_len == 0 { 0 } else { len.saturating_add(op_len) }
    }

    /// Emulacion MMIO minima para WHPX MemoryAccess: MOV r/m<->r (8/16/32/64).
    fn try_emulate_whpx_mmio(&mut self, fault_gpa: u64) -> Result<bool, String> {
        let mut regs = self.get_regs(); let rip = regs.rip;
        let mut bytes = [0u8; 15];
        for i in 0..15u64 { bytes[i as usize] = self.memory.read_u8(GuestAddress::new(rip.wrapping_add(i))).map_err(|e| e.to_string())?; }
        let mut i=0usize; let mut rex=0u8; let mut has_rex=false; let mut op16=false; let mut addr32=false;
        for _ in 0..6 {
            let b=bytes[i];
            match b { 0x66=>{op16=true;i+=1;} 0x67=>{addr32=true;i+=1;} 0xF0|0xF2|0xF3|0x2E|0x36|0x3E|0x26|0x64|0x65=>{i+=1;} 0x40..=0x4F=>{has_rex=true;rex=b;i+=1;} _=>break, }
            if i>=15 { return Ok(false); }
        }
        let rex_w=has_rex&&(rex&8)!=0; let rex_r=has_rex&&(rex&4)!=0; let rex_x=has_rex&&(rex&2)!=0; let rex_b=has_rex&&(rex&1)!=0;
        let opcode=bytes[i]; i+=1;
        let size = if matches!(opcode,0x88|0x8A){1u8} else if rex_w{8} else if op16{2} else {4};
        if !matches!(opcode,0x88|0x89|0x8A|0x8B) { return Ok(false); }
        let modrm=bytes[i]; i+=1; let modb=(modrm>>6)&3; let reg=((modrm>>3)&7)|if rex_r{8}else{0}; let rm=(modrm&7)|if rex_b{8}else{0};
        if modb==3 { return Ok(false); }
        let (addr, disp_len)=decode_modrm_addr(&regs,&bytes,i,modb,rm,rex_x,addr32); i+=disp_len;
        if addr!=fault_gpa && !self.devices.iter().any(|d| d.handles_mmio(addr)) { return Ok(false); }
        match opcode {
            0x89=>{ let v=get_reg_by_idx(&regs,reg,size); self.dispatch_mmio_write(addr,v,size); }
            0x8B=>{ let v=self.dispatch_mmio_read(addr,size).unwrap_or(0); set_reg_by_idx(&mut regs,reg,size,v); }
            0x88=>{ let v=get_reg_by_idx(&regs,reg,1); self.dispatch_mmio_write(addr,v,1); }
            0x8A=>{ let v=self.dispatch_mmio_read(addr,1).unwrap_or(0); set_reg_by_idx(&mut regs,reg,1,v); }
            _=>return Ok(false),
        }
        regs.rip = rip.wrapping_add(i as u64); self.set_regs(&regs); Ok(true)
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

    fn dispatch_mmio_read(&mut self, addr: u64, size: u8) -> Option<u64> {
        for dev in &mut self.devices {
            if dev.handles_mmio(addr) { return Some(dev.read_mmio(addr, size)); }
        }
        None
    }
    fn dispatch_mmio_write(&mut self, addr: u64, data: u64, size: u8) -> bool {
        for dev in &mut self.devices {
            if dev.handles_mmio(addr) { dev.write_mmio(addr, data, size); return true; }
        }
        false
    }

    fn deliver_pending_interrupt_software(&mut self) -> Result<(), VmmError> {
        if self.backend_kind != VmBackendKind::Software { return Ok(()); }
        // Check IF and PIC
        let (if_enabled, idtr_base, idtr_limit) = match &self.exec {
            ExecutionBackend::Software(cpu) => (cpu.regs.interrupts_enabled, cpu.regs.idtr.base, cpu.regs.idtr.limit),
            _ => return Ok(()),
        };
        if !if_enabled || idtr_base == 0 { return Ok(()); }
        // get vector from PIC
        let mut vector: Option<u8> = None;
        for dev in &mut self.devices {
            if let Some(pic) = dev.as_any_mut().downcast_mut::<Pic8259>() {
                vector = pic.get_interrupt();
                break;
            }
        }
        let Some(vec) = vector else { return Ok(()); };
        // deliver via CPU IDT
        let mut bus = VmBus { mem: &mut self.memory, devices: &mut self.devices };
        match &mut self.exec {
            ExecutionBackend::Software(cpu) => {
                if let Err(e) = cpu.deliver_external_interrupt(vec, &mut bus) {
                    // if IDT not configured, fall back to halting with diagnostic - but better keep running
                    let _ = e;
                }
            }
            _ => {}
        }
        let _ = idtr_limit;
        Ok(())
    }

    /// Ejecuta un paso de la VM.
    /// - Software: tick + una instrucción.
    /// - WHPX: tick + una ejecución hasta VM-exit (puede abarcar muchas instrucciones).
    pub fn step_once(&mut self) -> Result<Option<VcpuExit>, VmmError> {
        if self.status == VmStatus::Created || self.status == VmStatus::Paused {
            self.status = VmStatus::Running;
        }
        self.tick_platform();
        self.deliver_pending_interrupt_software()?;

        match self.backend_kind {
            VmBackendKind::Software => {
                let cpu_res = {
                    let mut bus = VmBus { mem: &mut self.memory, devices: &mut self.devices };
                    match &mut self.exec {
                        ExecutionBackend::Software(cpu) => cpu.step(&mut bus),
                        ExecutionBackend::Whpx(_) => return Err(VmmError::Unsupported),
                    }
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
                    Ok(Some(CpuExitReason::MmioRead { addr, size })) => {
                        // VmBus should have handled; report unhandled MMIO
                        Ok(Some(VcpuExit::MmioRead { addr, size }))
                    }
                    Ok(Some(CpuExitReason::MmioWrite { addr, data, size })) => {
                        Ok(Some(VcpuExit::MmioWrite { addr, data, size }))
                    }
                    Ok(Some(CpuExitReason::Interrupt { vector })) => Ok(Some(VcpuExit::Unknown(format!("interrupt 0x{vector:02X}")))),
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
                    match self.try_emulate_whpx_mmio(gpa) {
                        Ok(true) => { self.tick_platform(); return Ok(None); }
                        Ok(false) => {
                            self.status = VmStatus::Error;
                            return Err(VmmError::MemoryMappingFailed(format!("WHPX MMIO en GPA 0x{gpa:X}: instrucción no emulable/sin handler MMIO")));
                        }
                        Err(e) => { self.status = VmStatus::Error; return Err(VmmError::MemoryMappingFailed(format!("WHPX MMIO en GPA 0x{gpa:X}: {e}"))); }
                    }
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

fn read_cdrom_boot_image(disk: &mut dyn BlockDevice) -> Result<Vec<u8>, VmmError> {
    let ss = disk.sector_size() as usize;
    if ss != 2048 {
        return Err(VmmError::InitializationFailed("CD-ROM requiere sector 2048".to_string()));
    }
    let mut sector = vec![0u8; ss];
    // Try El Torito: Boot Record Volume Descriptor at LBA 17
    if disk.sector_count() > 17 {
        disk.read_sectors(17, 1, &mut sector).map_err(|e| VmmError::InitializationFailed(format!("ISO LBA17: {e}")))?;
        if sector[0] == 0 && &sector[1..6] == b"CD001" && sector[6] == 1
            && sector[7..39].starts_with(b"EL TORITO SPECIFICATION")
        {
            let catalog_lba = u32::from_le_bytes([sector[0x47], sector[0x48], sector[0x49], sector[0x4A]]) as u64;
            if catalog_lba < disk.sector_count() {
                let mut cat = vec![0u8; ss];
                disk.read_sectors(catalog_lba, 1, &mut cat).map_err(|e| VmmError::InitializationFailed(format!("El Torito catalog: {e}")))?;
                // Validation Entry at 0x00: header id 0x01, platform 0x00 (x86), 55AA at 0x1E
                let valid = cat[0] == 0x01 && cat[1] == 0x00 && cat[0x1E] == 0x55 && cat[0x1F] == 0xAA;
                // Initial/Default Entry at 0x20: bootable 0x88
                if valid && cat[0x20] == 0x88 {
                    let sector_count_512 = u16::from_le_bytes([cat[0x26], cat[0x27]]) as usize;
                    let load_rba = u32::from_le_bytes([cat[0x28], cat[0x29], cat[0x2A], cat[0x2B]]) as u64;
                    let bytes_to_load = if sector_count_512 == 0 { 512 } else { sector_count_512.saturating_mul(512) };
                    let sectors_2048 = ((bytes_to_load + 2047) / 2048).max(1) as u64;
                    if load_rba.saturating_add(sectors_2048) <= disk.sector_count() {
                        let mut img = vec![0u8; (sectors_2048 as usize) * ss];
                        disk.read_sectors(load_rba, sectors_2048 as u32, &mut img)
                            .map_err(|e| VmmError::InitializationFailed(format!("El Torito boot image: {e}")))?;
                        img.truncate(bytes_to_load.min(img.len()));
                        if img.len() >= 512 {
                            // Return up to 2048 bytes for 0x7C00 area (enough for boot sector + small stub)
                            let out_len = img.len().min(2048);
                            let mut out = vec![0u8; 2048];
                            out[..out_len].copy_from_slice(&img[..out_len]);
                            return Ok(out);
                        }
                    }
                }
            }
        }
    }
    // Fallback: LBA 0
    disk.read_sectors(0, 1, &mut sector).map_err(|e| VmmError::InitializationFailed(format!("CD-ROM LBA0: {e}")))?;
    Ok(sector)
}

/// Construye una imagen BIOS ROM de 64 KiB con un stub determinista (M12.1/M12.2).
/// - Mapeo esperado: 0xF0000..0xFFFFF (entrada de reset 0xFFFF0).
/// - M12.1: intenta boot ATA PIO desde IDE primario maestro (0x1F0), lee LBA0 a 0x7C00 y salta.
/// - M12.2: si ATA falla/no DRQ, intenta ATAPI PIO en IDE secundario maestro (0x170),
///   envía PACKET READ(10) LBA0 count=1 (2048 bytes) y carga a 0x7C00, luego salta.
/// Modelo plano/software CPU; no INT BIOS reales, no El Torito completo en ROM (la ruta El Torito
/// se resuelve por helper Rust `boot_from_cdrom_secondary_master` cuando aplica).
fn build_builtin_bios_stub() -> Vec<u8> {
    let mut rom = vec![0u8; 0x1_0000];
    let base: u32 = 0xF_0000;
    let mut code: Vec<u8> = Vec::new();

    fn patch_rel8(code: &mut [u8], pos: usize, target: usize) {
        let rel = (target as i32) - ((pos as i32) + 2);
        assert!(rel >= -128 && rel <= 127, "rel8 fuera de rango: {rel} pos={pos} target={target}");
        code[pos + 1] = rel as i8 as u8;
    }

    // init stack
    code.extend_from_slice(&[0xBC, 0xF0, 0xFF, 0x07, 0x00]); // mov esp,0x7FFF0
    for &b in b"CapsuleBIOS M12\r\n" {
        code.extend_from_slice(&[0xBA, 0xF8, 0x03, 0x00, 0x00, 0xB0, b, 0xEE]);
    }

    // ===== ATA primary master (0x1F0) =====
    code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]); // drive/head
    code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]); // count
    code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x20, 0xEE]); // READ SECTORS
    code.extend_from_slice(&[0xB9, 0x00, 0x00, 0x10, 0x00]); // timeout
    let poll_hdd = code.len();
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xEC]); // in al,dx
    code.extend_from_slice(&[0xA8, 0x21]); let jnz_hdd_err = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xA8, 0x08]); let jnz_hdd_drq = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xFF, 0xC9]);
    let rb = (poll_hdd as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rb as i8 as u8]);
    let jmp_try_cd = code.len();
    code.extend_from_slice(&[0xEB, 0x00]); // timeout -> try_cd

    // read_hdd close to poll_hdd for rel8
    let read_hdd = code.len();
    code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]); // data port
    code.extend_from_slice(&[0xBF, 0x00, 0x7C, 0x00, 0x00]); // edi
    code.extend_from_slice(&[0xB9, 0x80, 0x00, 0x00, 0x00]); // 128 dwords
    let rlh = code.len();
    code.extend_from_slice(&[0xED, 0x89, 0x07, 0x83, 0xC7, 0x04, 0xFF, 0xC9]);
    let rrh = (rlh as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rrh as i8 as u8]);
    code.extend_from_slice(&[0xB8, 0x00, 0x7C, 0x00, 0x00, 0xFF, 0xE0]); // jmp 0x7C00

    let try_cd = code.len();
    // ===== ATAPI secondary master (0x170) =====
    for &b in b"BIOS:CD\r\n" {
        code.extend_from_slice(&[0xBA, 0xF8, 0x03, 0x00, 0x00, 0xB0, b, 0xEE]);
    }
    code.extend_from_slice(&[0xBA, 0x76, 0x01, 0x00, 0x00, 0xB0, 0xA0, 0xEE]); // select
    code.extend_from_slice(&[0xBA, 0x71, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // features
    code.extend_from_slice(&[0xBA, 0x74, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // bytecount lo
    code.extend_from_slice(&[0xBA, 0x75, 0x01, 0x00, 0x00, 0xB0, 0x08, 0xEE]); // bytecount hi
    code.extend_from_slice(&[0xBA, 0x77, 0x01, 0x00, 0x00, 0xB0, 0xA0, 0xEE]); // PACKET
    code.extend_from_slice(&[0xB9, 0x00, 0x00, 0x10, 0x00]);
    let poll_pkt = code.len();
    code.extend_from_slice(&[0xBA, 0x77, 0x01, 0x00, 0x00, 0xEC]);
    code.extend_from_slice(&[0xA8, 0x21]); let jnz_cd_err = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xA8, 0x08]); let jnz_cd_drq = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xFF, 0xC9]);
    let rp2 = (poll_pkt as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rp2 as i8 as u8]);
    let jmp_cd_err1 = code.len();
    code.extend_from_slice(&[0xEB, 0x00]);

    let send_pkt = code.len();
    code.extend_from_slice(&[0xBA, 0x70, 0x01, 0x00, 0x00]); // dx=0x170
    for dw in [0x0000_0028u32, 0x0000_0000u32, 0x0000_0100u32] {
        let b = dw.to_le_bytes();
        code.extend_from_slice(&[0xB8, b[0], b[1], b[2], b[3], 0xEF]); // out dx,eax
    }
    code.extend_from_slice(&[0xB9, 0x00, 0x00, 0x10, 0x00]);
    let poll_cd_data = code.len();
    code.extend_from_slice(&[0xBA, 0x77, 0x01, 0x00, 0x00, 0xEC]);
    code.extend_from_slice(&[0xA8, 0x21]); let jnz_cd_data_err = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xA8, 0x08]); let jnz_cd_data_drq = code.len(); code.extend_from_slice(&[0x75, 0x00]);
    code.extend_from_slice(&[0xFF, 0xC9]);
    let rp3 = (poll_cd_data as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rp3 as i8 as u8]);
    let jmp_cd_err2 = code.len();
    code.extend_from_slice(&[0xEB, 0x00]);

    let read_cd = code.len();
    code.extend_from_slice(&[0xBA, 0x70, 0x01, 0x00, 0x00]); // dx=0x170
    code.extend_from_slice(&[0xBF, 0x00, 0x7C, 0x00, 0x00]); // edi=0x7C00
    code.extend_from_slice(&[0xB9, 0x00, 0x02, 0x00, 0x00]); // 512 dwords
    let rlc = code.len();
    code.extend_from_slice(&[0xED, 0x89, 0x07, 0x83, 0xC7, 0x04, 0xFF, 0xC9]);
    let rrc = (rlc as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rrc as i8 as u8]);
    code.extend_from_slice(&[0xB8, 0x00, 0x7C, 0x00, 0x00, 0xFF, 0xE0]);

    let cd_err = code.len();
    for &b in b"BIOS:NOBOOT\r\n" { code.extend_from_slice(&[0xBA, 0xF8, 0x03, 0x00, 0x00, 0xB0, b, 0xEE]); }
    code.push(0xF4);

    patch_rel8(&mut code, jnz_hdd_err, try_cd);
    patch_rel8(&mut code, jnz_hdd_drq, read_hdd);
    patch_rel8(&mut code, jmp_try_cd, try_cd);
    patch_rel8(&mut code, jnz_cd_err, cd_err);
    patch_rel8(&mut code, jnz_cd_drq, send_pkt);
    patch_rel8(&mut code, jmp_cd_err1, cd_err);
    patch_rel8(&mut code, jnz_cd_data_err, cd_err);
    patch_rel8(&mut code, jnz_cd_data_drq, read_cd);
    patch_rel8(&mut code, jmp_cd_err2, cd_err);

    let code_off = 0x0100usize;
    assert!(code_off + code.len() < rom.len());
    rom[code_off..code_off + code.len()].copy_from_slice(&code);
    let reset_off = 0xFFF0usize;
    let target = base + (code_off as u32);
    let next = base + 0xFFF5;
    let rel = (target as i32).wrapping_sub(next as i32);
    rom[reset_off] = 0xE9;
    rom[reset_off + 1..reset_off + 5].copy_from_slice(&rel.to_le_bytes());
    rom
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
        idtr: Default::default(),
        interrupts_enabled: (r.rflags & 0x200) != 0,
    }
}

fn decode_modrm_addr(regs: &Registers, bytes: &[u8;15], mut i: usize, modb: u8, rm: u8, rex_x: bool, addr32: bool) -> (u64, usize) {
    let mut disp_len=0usize; let mut addr:u64;
    let rm_low=rm&7;
    if rm_low==4 {
        let sib=bytes[i]; disp_len+=1;
        let scale=1u64<<((sib>>6)&3);
        let idx_raw=(sib>>3)&7; let base_raw=sib&7;
        let idx=idx_raw|if rex_x{8}else{0}; let base=base_raw|(rm&8);
        let base_val = if base_raw==5 && modb==0 { 0 } else { get_reg_by_idx(regs,base, if addr32{4}else{8}) };
        let idx_val = if idx_raw==4 && !rex_x { 0 } else { get_reg_by_idx(regs,idx, if addr32{4}else{8}) };
        addr = base_val.wrapping_add(idx_val.wrapping_mul(scale));
        if base_raw==5 && modb==0 { let d=i32::from_le_bytes(bytes[i+disp_len..i+disp_len+4].try_into().unwrap()) as i64; addr=(addr as i64).wrapping_add(d) as u64; disp_len+=4; }
    } else if rm_low==5 && modb==0 {
        let d=i32::from_le_bytes(bytes[i..i+4].try_into().unwrap()) as u64; addr=d; disp_len+=4;
    } else { addr = get_reg_by_idx(regs,rm, if addr32{4}else{8}); }
    if modb==1 { let d=bytes[i+disp_len] as i8 as i64; addr=(addr as i64).wrapping_add(d) as u64; disp_len+=1; }
    else if modb==2 { let d=i32::from_le_bytes(bytes[i+disp_len..i+disp_len+4].try_into().unwrap()) as i64; addr=(addr as i64).wrapping_add(d) as u64; disp_len+=4; }
    if addr32 { addr &= 0xFFFF_FFFF; }
    (addr, disp_len)
}
fn get_reg_by_idx(r: &Registers, idx: u8, size: u8) -> u64 {
    let v = match idx {0=>r.rax,1=>r.rcx,2=>r.rdx,3=>r.rbx,4=>r.rsp,5=>r.rbp,6=>r.rsi,7=>r.rdi,8=>r.r8,9=>r.r9,10=>r.r10,11=>r.r11,12=>r.r12,13=>r.r13,14=>r.r14,15=>r.r15,_=>0};
    match size {1=>v&0xFF,2=>v&0xFFFF,4=>v&0xFFFF_FFFF,8=>v,_=>v}
}
fn set_reg_by_idx(r: &mut Registers, idx: u8, size: u8, val: u64) {
    let cur=get_reg_by_idx(r,idx,8);
    let nv = match size {1=>(cur&!0xFF)|(val&0xFF),2=>(cur&!0xFFFF)|(val&0xFFFF),4=>val&0xFFFF_FFFF,8=>val,_=>val};
    match idx {0=>r.rax=nv,1=>r.rcx=nv,2=>r.rdx=nv,3=>r.rbx=nv,4=>r.rsp=nv,5=>r.rbp=nv,6=>r.rsi=nv,7=>r.rdi=nv,8=>r.r8=nv,9=>r.r9=nv,10=>r.r10=nv,11=>r.r11=nv,12=>r.r12=nv,13=>r.r13=nv,14=>r.r14=nv,15=>r.r15=nv,_=>{}}
}
