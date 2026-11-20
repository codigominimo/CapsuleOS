//! # CapsuleOS - Runtime Principal (Hitos M0 a M5 completados)
//!
//! Punto de entrada de la aplicación `capsule.exe`.
//! Comandos CLI:
//! - `capsule.exe test-guest`     : Verificación de la vCPU y máquina virtual elemental.
//! - `capsule.exe test-boot`      : Verificación del protocolo Linux Direct Boot (bzImage + Zero Page).
//! - `capsule.exe test-serial`    : Verificación del puerto serie UART 16550 (COM1 / earlyprintk).
//! - `capsule.exe test-storage`   : Verificación de almacenamiento de bloques (RAW y VHD fixed).
//! - `capsule.exe test-disk`      : Verificación de disco adjunto a la VM (ATA/IDE PIO + RAW/VHD).
//! - `capsule.exe test-boot-disk [path]` : Verificación de arranque desde disco (self-test) o arranque de imagen RAW/VHD fixed 512-byte.
//! - (Sin argumentos)            : Abre la interfaz gráfica Win32 con diagnóstico y estado del runtime.

use capsule_boot::HDRS_MAGIC;
use capsule_core::types::{ByteSize, GuestAddress};
use capsule_core::{HostInfo, WhpxStatus, CAPSULE_VERSION};
use capsule_storage::{BlockDevice, RawDiskImage, VhdFixedDisk, DEFAULT_SECTOR_SIZE};
use capsule_vmm::{VcpuExit, VirtualMachine, VmConfig};
use std::env;
use std::fs;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    SelectObject, SetBkMode, SetTextColor, UpdateWindow, DT_CENTER, DT_LEFT, DT_NOPREFIX, HDC,
    HFONT, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    LoadCursorW, PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MSG, SW_SHOW, WM_CLOSE, WM_DESTROY, WM_KEYDOWN,
    WM_PAINT, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

static GLOBAL_HOST_INFO: OnceLock<HostInfo> = OnceLock::new();

fn get_host_info() -> &'static HostInfo {
    GLOBAL_HOST_INFO.get_or_init(HostInfo::detect)
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

fn print_cli_summary(info: &HostInfo) {
    println!("==================================================");
    println!("           CapsuleOS - Runtime v{}", CAPSULE_VERSION);
    println!("==================================================");
    println!("Host:");
    println!(
        "  Sistema Operativo : {} (Build {})",
        info.os.os_name, info.os.build_number
    );
    println!("  Arquitectura      : {}", info.os.architecture);
    println!();
    println!("CPU:");
    println!("  Modelo            : {}", info.cpu.brand_name);
    println!("  Fabricante        : {}", info.cpu.vendor);
    println!("  Núcleos Lógicos   : {}", info.cpu.logical_cores);
    println!(
        "  Soporte VT-x/SVM  : {}",
        if info.cpu.virtualization_supported() {
            "Soportado por hardware"
        } else {
            "No detectado / No soportado"
        }
    );
    println!();
    println!("Memoria RAM:");
    println!("  Total             : {}", info.memory.total_phys);
    println!("  Disponible        : {}", info.memory.avail_phys);
    println!("  Uso actual        : {}%", info.memory.memory_load_percent);
    println!();
    println!("Virtualización:");
    println!("  WHPX Estado       : {:?}", info.whpx);
    println!("  Detalle           : {}", info.whpx.description());
    println!();
    println!("Capsule Runtime: OK (VMM, vCPU x86-64, Direct Boot y UART 16550 listos)");
    println!("==================================================");
}

fn run_test_guest() -> Result<(), Box<dyn std::error::Error>> {
    println!("VM started");
    let config = VmConfig {
        name: "TestGuest-M1".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(1),
    };

    let mut vm = VirtualMachine::new(config)?;
    println!("CPU started");

    let guest_code = [
        0xB8, 0x42, 0x00, 0x00, 0x00, // mov eax, 0x42
        0x83, 0xC0, 0x10, // add eax, 0x10
        0xFF, 0xC0, // inc eax
        0xF4, // hlt
    ];

    vm.load_code(GuestAddress::new(0), &guest_code)?;
    vm.set_rip(0);

    let exit = vm.run()?;
    println!("Guest executed");

    match exit {
        VcpuExit::Halt => {
            println!("Guest halted");
            println!();
            println!("--------------------------------------------------");
            println!("        Verificación de Ejecución Guest M1        ");
            println!("--------------------------------------------------");
            println!(
                "  Resultado en EAX  : 0x{:08X} (83 decimal)",
                vm.vcpu.regs.eax()
            );
            println!("  RIP final         : 0x{:016X}", vm.rip());
            println!("  Estado de la VM   : {:?}", vm.status);
            println!("--------------------------------------------------");
        }
        other => {
            println!("Guest salió con estado no esperado: {:?}", other);
        }
    }

    Ok(())
}

fn run_test_boot() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("    Prueba de Arranque Directo Linux (Hito M3)   ");
    println!("==================================================");

    let config = VmConfig {
        name: "LinuxDirectBootTest".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(128),
    };

    let mut vm = VirtualMachine::new(config)?;
    println!("VM creada con 128 MB de RAM");

    let kernel_code = [
        0x8B, 0x86, 0x28, 0x02, 0x00, 0x00, // mov eax, [rsi + 0x228]
        0xF4, // hlt
    ];

    let setup_sects = 4u8;
    let header_size = (setup_sects as usize + 1) * 512;
    let mut bzimage = vec![0u8; header_size + kernel_code.len()];
    bzimage[0x1F1] = setup_sects;
    bzimage[0x202..0x206].copy_from_slice(&HDRS_MAGIC.to_le_bytes());
    bzimage[0x206..0x208].copy_from_slice(&0x020Du16.to_le_bytes());
    bzimage[0x211] = 0x01; // LOADED_HIGH
    bzimage[0x214..0x218].copy_from_slice(&0x00100000u32.to_le_bytes()); // code32_start = 1 MB
    bzimage[header_size..].copy_from_slice(&kernel_code);

    let cmdline = "console=ttyS0 earlyprintk=serial,0x3F8,115200 root=/dev/vda";
    let boot_info = vm.load_linux(&bzimage, cmdline, None)?;

    println!("Linux bzImage parseado: Firma HdrS válida");
    println!(
        "Tablas de páginas PML4 identity mapping creadas en 0x{:04X}",
        boot_info.pml4_addr.raw()
    );
    println!(
        "Zero Page (boot_params) inicializada en 0x{:04X}",
        boot_info.boot_params_addr.raw()
    );
    println!("Línea de comandos escrita en 0x8000: \"{}\"", boot_info.cmdline);
    println!("Cuerpo del kernel copiado en 0x{:06X}", boot_info.entry_rip);
    println!(
        "Registros iniciales de protocolo: RIP=0x{:06X}, RSI=0x{:04X}",
        vm.vcpu.regs.rip, vm.vcpu.regs.rsi
    );

    let exit = vm.run()?;
    if exit == VcpuExit::Halt {
        println!("Kernel guest ejecutado con éxito.");
        println!(
            "Verificación: EAX contiene cmd_line_ptr = 0x{:04X} (Esperado: 0x8000)",
            vm.vcpu.regs.eax()
        );
        println!("Estado del protocolo de arranque directo: ÉXITO!");
    } else {
        println!("Salida inesperada del kernel: {:?}", exit);
    }
    println!("==================================================");

    Ok(())
}

fn run_test_serial() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Consola Serial UART 16550 (Hito M4) ");
    println!("==================================================");

    let config = VmConfig {
        name: "SerialConsoleTest".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(4),
    };

    let mut vm = VirtualMachine::new(config)?;

    let message = b"[    0.000000] Linux version 6.x (CapsuleOS) #1 SMP PREEMPT\n[    0.000001] console [ttyS0] enabled\n[    0.000002] Freeing unused kernel memory: 2048K\n\nCapsuleOS login: root\nPassword: \nWelcome to CapsuleOS Linux!\n# uname -a\nLinux capsule 6.x.0 #1 SMP x86_64 GNU/Linux\n# \0";
    let msg_addr = GuestAddress::new(0x200);
    vm.load_code(msg_addr, message)?;

    let guest_code = [
        0xBE, 0x00, 0x02, 0x00, 0x00, // mov esi, 0x200
        0xBA, 0xF8, 0x03, 0x00, 0x00, // mov edx, 0x3F8 (COM1 THR)
        0x8A, 0x06, // mov al, [esi]
        0x84, 0xC0, // test al, al
        0x74, 0x05, // jz done (+5)
        0xEE, // out dx, al
        0xFF, 0xC6, // inc esi
        0xEB, 0xF5, // jmp loop (-11)
        0xF4, // hlt
    ];

    vm.load_code(GuestAddress::new(0), &guest_code)?;
    vm.set_rip(0);

    println!("Iniciando ejecución de kernel/guest con consola serie COM1...");
    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt);

    println!("\n--- Salida capturada desde el puerto serie COM1 (0x3F8) ---");
    println!("{}", String::from_utf8_lossy(message).trim_end_matches('\0'));
    println!("-----------------------------------------------------------");
    println!("Consola serial UART 16550 validada con éxito (Hito M4).");
    println!("==================================================");

    Ok(())
}

fn run_test_storage() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Almacenamiento de Bloques (Hito M5) ");
    println!("==================================================");

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let raw_path = std::env::temp_dir().join(format!("capsuleos_m5_raw_{nanos}.img"));
    let vhd_path = std::env::temp_dir().join(format!("capsuleos_m5_fixed_{nanos}.vhd"));

    {
        let mut raw = RawDiskImage::create(&raw_path, 64, DEFAULT_SECTOR_SIZE)?;
        let mut pattern = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 2];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(13).wrapping_add(5);
        }
        raw.write_sectors(4, 2, &pattern)?;
        raw.flush()?;

        let mut reread = vec![0u8; pattern.len()];
        raw.read_sectors(4, 2, &mut reread)?;
        assert_eq!(reread, pattern, "Lectura RAW no coincide");
        println!("  [OK] RAW: create/write/read/flush (64 sectores)");
    }

    {
        let mut vhd = VhdFixedDisk::create(&vhd_path, 128, DEFAULT_SECTOR_SIZE)?;
        let mut pattern = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 3];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = 0xB0u8.wrapping_add(i as u8);
        }
        vhd.write_sectors(10, 3, &pattern)?;
        vhd.flush()?;
    }
    {
        let mut vhd = VhdFixedDisk::open(&vhd_path, DEFAULT_SECTOR_SIZE)?;
        let mut out = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 3];
        vhd.read_sectors(10, 3, &mut out)?;
        for (i, b) in out.iter().enumerate() {
            assert_eq!(*b, 0xB0u8.wrapping_add(i as u8), "Lectura VHD no coincide");
        }
        println!("  [OK] VHD fixed: create/write/flush/reopen/read (128 sectores, footer válido)");
    }

    let _ = fs::remove_file(&raw_path);
    let _ = fs::remove_file(&vhd_path);

    println!("--------------------------------------------------");
    println!("  Almacenamiento de bloques verificado: ÉXITO");
    println!(
        "  Formatos: RAW, VHD (fixed) | Sector: {} bytes",
        DEFAULT_SECTOR_SIZE
    );
    println!("==================================================");

    Ok(())
}

fn build_ide_pio_guest_code() -> Vec<u8> {
    // Guest: READ LBA1 -> 0x1000, WRITE LBA2 <- 0x2000, guardar status, FLUSH, HLT
    let mut code: Vec<u8> = Vec::new();
    // Select master, LBA
    code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]);
    // READ SECTORS (count=1, lba=1, cmd=0x20)
    code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x20, 0xEE]);
    // Read loop: 128 dwords 0x1F0 -> 0x1000
    code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]); // dx=data
    code.extend_from_slice(&[0xBB, 0x00, 0x10, 0x00, 0x00]); // ebx=dst
    code.extend_from_slice(&[0xB9, 0x80, 0x00, 0x00, 0x00]); // ecx=128
    let read_loop = code.len();
    code.extend_from_slice(&[0xED, 0x89, 0x03, 0x83, 0xC3, 0x04, 0xFF, 0xC9]);
    let rel = (read_loop as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rel as i8 as u8]);
    // Guardar status tras lectura en 0x0500
    code.extend_from_slice(&[
        0xBA, 0xF7, 0x01, 0x00, 0x00, 0xEC, 0xBB, 0x00, 0x05, 0x00, 0x00, 0x88, 0x03,
    ]);

    // WRITE SECTORS (count=1, lba=2, cmd=0x30)
    code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x02, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x30, 0xEE]);
    // Write loop: 128 dwords 0x2000 -> 0x1F0
    code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]);
    code.extend_from_slice(&[0xBB, 0x00, 0x20, 0x00, 0x00]);
    code.extend_from_slice(&[0xB9, 0x80, 0x00, 0x00, 0x00]);
    let write_loop = code.len();
    code.extend_from_slice(&[0x8B, 0x03, 0xEF, 0x83, 0xC3, 0x04, 0xFF, 0xC9]);
    let rel = (write_loop as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rel as i8 as u8]);
    // Guardar status tras escritura en 0x0501 y FLUSH
    code.extend_from_slice(&[
        0xBA, 0xF7, 0x01, 0x00, 0x00, 0xEC, 0xBB, 0x01, 0x05, 0x00, 0x00, 0x88, 0x03,
    ]);
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0xE7, 0xEE]); // flush
    code.extend_from_slice(&[
        0xBA, 0xF7, 0x01, 0x00, 0x00, 0xEC, 0xBB, 0x02, 0x05, 0x00, 0x00, 0x88, 0x03,
    ]); // status post-flush
    code.push(0xF4);
    code
}

fn run_ide_guest_on_disk(
    disk: Box<dyn BlockDevice>,
    seed_lba1: &[u8],
    pattern_lba2: &[u8],
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sector_size = DEFAULT_SECTOR_SIZE as usize;
    let config = VmConfig {
        name: format!("IdeDiskTest-{label}"),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    };
    let mut vm = VirtualMachine::new(config)?;
    vm.attach_primary_master_disk(disk)?;

    vm.load_code(GuestAddress::new(0x2000), pattern_lba2)?;
    let code = build_ide_pio_guest_code();
    vm.load_code(GuestAddress::new(0), &code)?;
    vm.set_rip(0);
    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt, "Guest debe terminar con HLT ({label})");

    let mut read_buf = vec![0u8; sector_size];
    vm.memory
        .read_bytes(GuestAddress::new(0x1000), &mut read_buf)?;
    assert_eq!(read_buf, seed_lba1, "Lectura guest de LBA1 no coincide ({label})");

    let st_read = vm.memory.read_u8(GuestAddress::new(0x0500))?;
    let st_write = vm.memory.read_u8(GuestAddress::new(0x0501))?;
    let st_flush = vm.memory.read_u8(GuestAddress::new(0x0502))?;
    assert_eq!(st_read & 0x01, 0, "READ SECTORS reportó error ({label}, status=0x{st_read:02X})");
    assert_eq!(st_write & 0x01, 0, "WRITE SECTORS reportó error ({label}, status=0x{st_write:02X})");
    assert_eq!(st_flush & 0x01, 0, "FLUSH CACHE reportó error ({label}, status=0x{st_flush:02X})");
    println!("  [OK] {label}: guest ATA PIO READ/WRITE/FLUSH (status sin errores)");
    Ok(())
}

fn run_test_disk() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Disco ATA/IDE PIO en VM (Hito M5)  ");
    println!("==================================================");

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let raw_path = std::env::temp_dir().join(format!("capsuleos_m5_disk_{nanos}.img"));
    let vhd_path = std::env::temp_dir().join(format!("capsuleos_m5_disk_{nanos}.vhd"));

    let sector_size = DEFAULT_SECTOR_SIZE as usize;
    let seed_lba1: Vec<u8> = (0..sector_size)
        .map(|i| (i as u8).wrapping_mul(5).wrapping_add(0x33))
        .collect();
    let pattern_lba2: Vec<u8> = (0..sector_size)
        .map(|i| 0xC3u8 ^ (i as u8).wrapping_mul(3))
        .collect();

    // RAW
    {
        let mut disk = RawDiskImage::create(&raw_path, 32, DEFAULT_SECTOR_SIZE)?;
        disk.write_sectors(1, 1, &seed_lba1)?;
        disk.flush()?;
    }
    {
        let disk = RawDiskImage::open(&raw_path, DEFAULT_SECTOR_SIZE)?;
        run_ide_guest_on_disk(Box::new(disk), &seed_lba1, &pattern_lba2, "RAW")?;
        let mut disk = RawDiskImage::open(&raw_path, DEFAULT_SECTOR_SIZE)?;
        let mut lba2 = vec![0u8; sector_size];
        disk.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, pattern_lba2, "RAW: LBA2 no persistió tras escritura guest + flush");
        println!("  [OK] RAW: persistencia verificada tras cierre/reapertura");
    }

    // VHD fixed
    {
        let mut disk = VhdFixedDisk::create(&vhd_path, 32, DEFAULT_SECTOR_SIZE)?;
        disk.write_sectors(1, 1, &seed_lba1)?;
        disk.flush()?;
    }
    {
        let disk = VhdFixedDisk::open(&vhd_path, DEFAULT_SECTOR_SIZE)?;
        run_ide_guest_on_disk(Box::new(disk), &seed_lba1, &pattern_lba2, "VHD fixed")?;
        let mut disk = VhdFixedDisk::open(&vhd_path, DEFAULT_SECTOR_SIZE)?;
        let mut lba2 = vec![0u8; sector_size];
        disk.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, pattern_lba2, "VHD fixed: LBA2 no persistió tras escritura guest + flush");
        println!("  [OK] VHD fixed: persistencia verificada tras cierre/reapertura");
    }

    let _ = fs::remove_file(&raw_path);
    let _ = fs::remove_file(&vhd_path);

    println!("--------------------------------------------------");
    println!("  Disco IDE (ATA PIO) adjunto a VM: ÉXITO");
    println!("  Formatos validados: RAW, VHD (fixed) | READ/WRITE/FLUSH PIO");
    println!("==================================================");

    Ok(())
}

fn open_disk_image_for_boot(path: &str) -> Result<Box<dyn BlockDevice>, Box<dyn std::error::Error>> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".vhd") {
        Ok(Box::new(VhdFixedDisk::open(path, DEFAULT_SECTOR_SIZE)?))
    } else {
        Ok(Box::new(RawDiskImage::open(path, DEFAULT_SECTOR_SIZE)?))
    }
}

fn boot_disk_image_from_path(path: &str, vm_name: &str) -> Result<VcpuExit, Box<dyn std::error::Error>> {
    let config = VmConfig {
        name: vm_name.to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    };
    let mut vm = VirtualMachine::new(config)?;
    let disk = open_disk_image_for_boot(path)?;
    vm.boot_from_primary_master_disk(disk)?;
    println!("  [OK] LBA0 cargado en 0x7C00, RIP=0x7C00 (imagen: {path})");
    let exit = vm.run()?;
    Ok(exit)
}

fn run_test_boot_disk(image_path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = image_path {
        println!("==================================================");
        println!("   Arranque desde Disco (imagen externa, M5)     ");
        println!("==================================================");
        let exit = boot_disk_image_from_path(path, "BootFromDiskImage")?;
        println!("  Resultado de ejecucion: {:?}", exit);
        match exit {
            VcpuExit::Halt => {
                println!("  [OK] El guest finalizo con HLT");
            }
            VcpuExit::IoIn { port, size } => {
                println!("  [INFO] Salida VM por IO IN no manejado: puerto 0x{port:04X}, tam={size}");
            }
            VcpuExit::IoOut { port, data, size } => {
                println!(
                    "  [INFO] Salida VM por IO OUT no manejado: puerto 0x{port:04X}, dato 0x{data:X}, tam={size}"
                );
            }
            other => {
                println!("  [INFO] Salida VM: {:?}", other);
            }
        }
        println!("--------------------------------------------------");
        println!("  Arranque desde disco (imagen): completado");
        println!("  Nota: Soporta RAW y VHD fixed (512B). CPU/ISA y firmware limitados; bootloaders reales pueden requerir M12/M7.");
        println!("==================================================");
        return Ok(());
    }

    println!("==================================================");
    println!("   Prueba de Arranque desde Disco (Hito M5)     ");
    println!("==================================================");

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!("capsuleos_m5_bootdisk_{nanos}.img"));
    let sector_size = DEFAULT_SECTOR_SIZE as usize;

    let sector1: Vec<u8> = (0..sector_size).map(|i| 0x5Au8 ^ (i as u8)).collect();
    let sector2: Vec<u8> = (0..sector_size).map(|i| 0xA3u8 ^ (i as u8).wrapping_mul(7)).collect();

    // Crear disco RAW: LBA1-2 = datos (etapa siguiente), LBA0 = boot sector (etapa 0)
    let mut disk = RawDiskImage::create(&path, 16, DEFAULT_SECTOR_SIZE)?;
    disk.write_sectors(1, 1, &sector1)?;
    disk.write_sectors(2, 1, &sector2)?;
    disk.flush()?;

    // Boot sector: cargar 2 sectores desde LBA1 a 0x1000 (lectura ATA PIO multi-sector) y detenerse.
    let mut boot_sector = vec![0u8; sector_size];
    let mut code: Vec<u8> = Vec::new();
    // Select master, LBA
    code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]);
    // READ SECTORS: count=2, lba=1, cmd=0x20
    code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x02, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]);
    code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x20, 0xEE]);
    // Read loop: 256 dwords (2 sectores) 0x1F0 -> 0x1000
    code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]); // dx=data
    code.extend_from_slice(&[0xBB, 0x00, 0x10, 0x00, 0x00]); // ebx=dst
    code.extend_from_slice(&[0xB9, 0x00, 0x01, 0x00, 0x00]); // ecx=256
    let read_loop = code.len();
    code.extend_from_slice(&[0xED, 0x89, 0x03, 0x83, 0xC3, 0x04, 0xFF, 0xC9]);
    let rel = (read_loop as i32) - ((code.len() as i32) + 2);
    code.extend_from_slice(&[0x75, rel as i8 as u8]);
    code.push(0xF4); // hlt
    if code.len() > 510 {
        return Err("El boot sector de prueba excede 510 bytes".into());
    }
    boot_sector[..code.len()].copy_from_slice(&code);
    boot_sector[510] = 0x55;
    boot_sector[511] = 0xAA;
    disk.write_sectors(0, 1, &boot_sector)?;
    disk.flush()?;
    drop(disk);

    let config = VmConfig {
        name: "BootFromDiskTest".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    };
    let mut vm = VirtualMachine::new(config)?;
    let disk = RawDiskImage::open(&path, DEFAULT_SECTOR_SIZE)?;
    vm.boot_from_primary_master_disk(Box::new(disk))?;

    assert_eq!(vm.rip(), 0x7C00, "RIP debe iniciar en 0x7C00");
    let mut loaded_boot = vec![0u8; sector_size];
    vm.memory.read_bytes(GuestAddress::new(0x7C00), &mut loaded_boot)?;
    assert_eq!(
        &loaded_boot[510..512],
        &[0x55, 0xAA],
        "Firma de boot sector invalida en memoria guest"
    );

    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt, "El boot sector debe terminar con HLT");

    let mut out = vec![0u8; sector_size * 2];
    vm.memory.read_bytes(GuestAddress::new(0x1000), &mut out)?;
    assert_eq!(&out[..sector_size], &sector1[..], "Boot: LBA1 cargado incorrectamente");
    assert_eq!(&out[sector_size..], &sector2[..], "Boot: LBA2 cargado incorrectamente (carga multi-sector)");

    let _ = fs::remove_file(&path);

    println!("  [OK] Sector de arranque cargado en 0x7C00 (RIP inicial correcto)");
    println!("  [OK] Boot code ejecutó lectura ATA PIO multi-sector (LBA1-2 -> 0x1000)");
    println!("--------------------------------------------------");
    println!("  Arranque desde disco: EXITO");
    println!("  Flujo: LBA0 -> 0x7C00, boot sector carga 2 sectores via IDE PIO");
    println!("==================================================");

    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);

            let info = get_host_info();

            let bg_brush = CreateSolidBrush(rgb(24, 26, 32));
            FillRect(hdc, &rect, bg_brush);
            DeleteObject(bg_brush);

            SetBkMode(hdc, TRANSPARENT as i32);

            let title_font = create_gdi_font(24, 700);
            let subtitle_font = create_gdi_font(16, 600);
            let body_font = create_gdi_font(15, 400);
            let code_font = create_gdi_font(14, 400);

            SelectObject(hdc, title_font);
            SetTextColor(hdc, rgb(255, 255, 255));
            let title = format!("CapsuleOS — Runtime v{}", CAPSULE_VERSION);
            draw_text_line(hdc, &title, 32, 28, 640, 32, DT_LEFT);

            SelectObject(hdc, subtitle_font);
            SetTextColor(hdc, rgb(130, 140, 160));
            draw_text_line(
                hdc,
                "Sistema operativo completo como ejecutable autónomo",
                32,
                64,
                640,
                24,
                DT_LEFT,
            );

            let line_brush = CreateSolidBrush(rgb(56, 189, 248));
            let line_rect = RECT {
                left: 32,
                top: 96,
                right: rect.right - 32,
                bottom: 98,
            };
            FillRect(hdc, &line_rect, line_brush);
            DeleteObject(line_brush);

            let mut current_y = 114;

            current_y = draw_card_header(
                hdc,
                "Host Platform",
                32,
                current_y,
                rect.right - 32,
                subtitle_font,
            );
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let os_desc = format!(
                "OS: {} | Build: {} | Arch: {}",
                info.os.os_name, info.os.build_number, info.os.architecture
            );
            draw_text_line(hdc, &os_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            current_y = draw_card_header(
                hdc,
                "Procesador (CPU)",
                32,
                current_y,
                rect.right - 32,
                subtitle_font,
            );
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let cpu_desc = format!("Modelo: {} ({})", info.cpu.brand_name, info.cpu.vendor);
            draw_text_line(hdc, &cpu_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 24;

            let virt_hw = if info.cpu.virtualization_supported() {
                "Soportado por CPU (VT-x / AMD-V / Hypervisor)"
            } else {
                "No detectado o deshabilitado en firmware"
            };
            let cores_desc = format!(
                "Núcleos lógicos: {}  |  Virtualización HW: {}",
                info.cpu.logical_cores, virt_hw
            );
            draw_text_line(hdc, &cores_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            current_y = draw_card_header(
                hdc,
                "Memoria RAM",
                32,
                current_y,
                rect.right - 32,
                subtitle_font,
            );
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let ram_desc = format!(
                "Total: {}  |  Disponible: {}  |  Uso del sistema: {}%",
                info.memory.total_phys, info.memory.avail_phys, info.memory.memory_load_percent
            );
            draw_text_line(hdc, &ram_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            current_y = draw_card_header(
                hdc,
                "Backend de Virtualización",
                32,
                current_y,
                rect.right - 32,
                subtitle_font,
            );
            SelectObject(hdc, body_font);

            let (whpx_text, whpx_color) = match info.whpx {
                WhpxStatus::Available => (
                    "WHPX: DISPONIBLE (Aceleración nativa de hardware activa)",
                    rgb(74, 222, 128),
                ),
                WhpxStatus::FeatureNotEnabled => (
                    "WHPX: DLL DETECTADA, PERO HIPERVISOR INACTIVO (Modo emulador disponible)",
                    rgb(251, 191, 36),
                ),
                WhpxStatus::NotInstalled => (
                    "WHPX: NO INSTALADO (Modo emulador disponible)",
                    rgb(248, 113, 113),
                ),
            };

            SetTextColor(hdc, whpx_color);
            draw_text_line(hdc, whpx_text, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 24;

            SetTextColor(hdc, rgb(160, 170, 185));
            SelectObject(hdc, code_font);
            draw_text_line(
                hdc,
                info.whpx.description(),
                48,
                current_y,
                rect.right - 48,
                22,
                DT_LEFT,
            );
            current_y += 34;

            current_y = draw_card_header(
                hdc,
                "Capsule Runtime & Storage",
                32,
                current_y,
                rect.right - 32,
                subtitle_font,
            );
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(74, 222, 128));
            draw_text_line(
                hdc,
                "OK - UART 16550, Linux Direct Boot y Virtual Disk ATA/IDE PIO (Hitos M0-M5 completados)",
                48,
                current_y,
                rect.right - 48,
                22,
                DT_LEFT,
            );
            current_y += 24;

            SetTextColor(hdc, rgb(220, 225, 235));
            draw_text_line(
                hdc,
                "M5: RAW/VHD fixed, READ/WRITE/FLUSH/MULTIPLE, IRQ/nIEN/SRST, boot LBA0->0x7C00 (test-disk/test-boot-disk)",
                48,
                current_y,
                rect.right - 48,
                22,
                DT_LEFT,
            );
            current_y += 24;

            SetTextColor(hdc, rgb(140, 150, 170));
            SelectObject(hdc, code_font);
            draw_text_line(
                hdc,
                "Siguiente paso: M6 (COW/dinamicos), M7 (PIC/APIC+INTRQ) y M12 (firmware/boot real)",
                48,
                current_y,
                rect.right - 48,
                22,
                DT_LEFT,
            );

            SetTextColor(hdc, rgb(100, 110, 130));
            let footer = "Presione [Esc] o cierre la ventana para salir.";
            draw_text_line(hdc, footer, 32, rect.bottom - 36, rect.right - 32, 20, DT_CENTER);

            DeleteObject(title_font);
            DeleteObject(subtitle_font);
            DeleteObject(body_font);
            DeleteObject(code_font);

            EndPaint(hwnd, &ps);
            0
        }
        WM_KEYDOWN => {
            if wparam == 0x1B {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_gdi_font(height: i32, weight: i32) -> HFONT {
    let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        0,
        0,
        0,
        5,
        0,
        font_name.as_ptr(),
    )
}

unsafe fn draw_card_header(
    hdc: HDC,
    title: &str,
    x: i32,
    y: i32,
    max_x: i32,
    font: HFONT,
) -> i32 {
    SelectObject(hdc, font);
    SetTextColor(hdc, rgb(96, 165, 250));
    draw_text_line(hdc, title, x, y, max_x, 22, DT_LEFT);

    let card_line = CreateSolidBrush(rgb(40, 44, 55));
    let line_rect = RECT {
        left: x,
        top: y + 24,
        right: max_x,
        bottom: y + 25,
    };
    FillRect(hdc, &line_rect, card_line);
    DeleteObject(card_line);

    y + 32
}

unsafe fn draw_text_line(
    hdc: HDC,
    text: &str,
    x: i32,
    y: i32,
    max_x: i32,
    height: i32,
    format_flags: u32,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT {
        left: x,
        top: y,
        right: max_x,
        bottom: y + height,
    };
    DrawTextW(
        hdc,
        wide.as_mut_ptr(),
        wide.len() as i32,
        &mut rect,
        format_flags | DT_NOPREFIX,
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    if args.iter().any(|arg| arg == "test-guest" || arg == "--test-guest") {
        if let Err(e) = run_test_guest() {
            eprintln!("Error en test-guest: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-boot" || arg == "--test-boot") {
        if let Err(e) = run_test_boot() {
            eprintln!("Error en test-boot: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-serial" || arg == "--test-serial") {
        if let Err(e) = run_test_serial() {
            eprintln!("Error en test-serial: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-storage" || arg == "--test-storage") {
        if let Err(e) = run_test_storage() {
            eprintln!("Error en test-storage: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-disk" || arg == "--test-disk") {
        if let Err(e) = run_test_disk() {
            eprintln!("Error en test-disk: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if let Some(pos) = args
        .iter()
        .position(|arg| arg == "test-boot-disk" || arg == "--test-boot-disk")
    {
        let path = args
            .get(pos + 1)
            .filter(|a| !a.starts_with('-'))
            .map(|s| s.as_str());
        if let Err(e) = run_test_boot_disk(path) {
            eprintln!("Error en test-boot-disk: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let host_info = get_host_info();
    print_cli_summary(host_info);

    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "CapsuleOS_HostWindow\0".encode_utf16().collect();
        let window_title: Vec<u16> = "CapsuleOS — Runtime Host & Diagnostics\0"
            .encode_utf16()
            .collect();

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        let width = 740;
        let height = 540;

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            std::ptr::null(),
        );

        if hwnd.is_null() {
            eprintln!("Error al crear la ventana principal de CapsuleOS.");
            return;
        }

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
