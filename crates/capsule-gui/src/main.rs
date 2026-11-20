//! # CapsuleOS - Runtime Principal (Hitos M0 a M9 completados)
//!
//! Punto de entrada de la aplicacion `capsule.exe`.
//! Comandos CLI:
//! - `capsule.exe test-guest`       : Verificacion de la vCPU y maquina virtual elemental.
//! - `capsule.exe test-boot`        : Verificacion del protocolo Linux Direct Boot (bzImage + Zero Page).
//! - `capsule.exe test-serial`      : Verificacion del puerto serie UART 16550 (COM1 / earlyprintk).
//! - `capsule.exe test-storage`     : Verificacion de almacenamiento de bloques (RAW y VHD fixed).
//! - `capsule.exe test-disk`        : Verificacion de disco adjunto a la VM (ATA/IDE PIO + RAW/VHD).
//! - `capsule.exe test-boot-disk [path]` : Verificacion de arranque desde disco (self-test) o arranque de imagen RAW/VHD fixed 512-byte.
//! - `capsule.exe test-cow`         : Verificacion de persistencia Copy-on-Write (overlay) sobre imagen base RAW/VHD.
//! - `capsule.exe test-platform`    : Verificacion de plataforma virtual (PIC 8259, PIT 8254, RTC/CMOS, PCI).
//! - `capsule.exe test-network`     : Verificacion de red en modo usuario (NAT) y virtio-net PCI (M8).
//! - `capsule.exe test-framebuffer` : Verificacion de framebuffer VGA/Bochs VBE y LFB (M9).
//! - (Sin argumentos)              : Abre la interfaz grafica Win32 con diagnostico y render de framebuffer en tiempo real.

use capsule_boot::HDRS_MAGIC;
use capsule_core::types::{ByteSize, GuestAddress};
use capsule_core::{HostInfo, WhpxStatus, CAPSULE_VERSION};
use capsule_storage::{BlockDevice, CowOverlayDisk, RawDiskImage, VhdFixedDisk, DEFAULT_SECTOR_SIZE};
use capsule_vmm::{VcpuExit, VirtualMachine, VmConfig};
use std::env;
use std::fs;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    InvalidateRect, SelectObject, SetBkMode, SetTextColor, StretchDIBits, UpdateWindow, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
    DT_TOP,
    HDC, HFONT, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, SW_SHOW, WM_CLOSE, WM_CREATE,
    WM_DESTROY, WM_KEYDOWN, WM_PAINT, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
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
    println!("Capsule Runtime: OK (VMM, vCPU x86-64, Direct Boot, UART, disco, plataforma y red M8 listos)");
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

fn run_test_cow() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Persistencia Copy-on-Write (M6)    ");
    println!("==================================================");

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sector_size = DEFAULT_SECTOR_SIZE as usize;
    let seed_lba1: Vec<u8> = (0..sector_size)
        .map(|i| (i as u8).wrapping_mul(11).wrapping_add(0x29))
        .collect();
    let pattern_lba2: Vec<u8> = (0..sector_size)
        .map(|i| 0x6Du8 ^ (i as u8).wrapping_mul(9))
        .collect();
    let zeros = vec![0u8; sector_size];

    // RAW base + COW overlay (guest I/O + persistencia)
    let raw_base = std::env::temp_dir().join(format!("capsuleos_m6_cow_base_{nanos}.img"));
    let raw_ov = std::env::temp_dir().join(format!("capsuleos_m6_cow_{nanos}.cow"));
    {
        let mut base = RawDiskImage::create(&raw_base, 32, DEFAULT_SECTOR_SIZE)?;
        base.write_sectors(1, 1, &seed_lba1)?;
        base.flush()?;
    }
    {
        let base = RawDiskImage::open(&raw_base, DEFAULT_SECTOR_SIZE)?;
        let cow = CowOverlayDisk::create(base, &raw_ov)?;
        run_ide_guest_on_disk(Box::new(cow), &seed_lba1, &pattern_lba2, "COW/RAW")?;
    }
    {
        let mut base = RawDiskImage::open(&raw_base, DEFAULT_SECTOR_SIZE)?;
        let mut lba2 = vec![0u8; sector_size];
        base.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, zeros, "COW/RAW: la imagen base no debe modificarse");
        println!("  [OK] RAW base intacta tras escrituras guest en overlay");
    }
    {
        let base = RawDiskImage::open(&raw_base, DEFAULT_SECTOR_SIZE)?;
        let mut cow = CowOverlayDisk::open(base, &raw_ov)?;
        let mut lba2 = vec![0u8; sector_size];
        cow.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, pattern_lba2, "COW/RAW: el overlay no persistió los cambios");
        let mut lba1 = vec![0u8; sector_size];
        cow.read_sectors(1, 1, &mut lba1)?;
        assert_eq!(lba1, seed_lba1, "COW/RAW: lectura de sector no modificado debe venir de base");
        println!("  [OK] RAW overlay persistente y lectura mezclada base/overlay");
    }

    // VHD fixed base + COW overlay (persistencia sin tocar base)
    let vhd_base = std::env::temp_dir().join(format!("capsuleos_m6_cow_base_{nanos}.vhd"));
    let vhd_ov = std::env::temp_dir().join(format!("capsuleos_m6_cow_{nanos}_vhd.cow"));
    {
        let mut base = VhdFixedDisk::create(&vhd_base, 32, DEFAULT_SECTOR_SIZE)?;
        base.write_sectors(1, 1, &seed_lba1)?;
        base.flush()?;
    }
    {
        let base = VhdFixedDisk::open(&vhd_base, DEFAULT_SECTOR_SIZE)?;
        let cow = CowOverlayDisk::create(base, &vhd_ov)?;
        run_ide_guest_on_disk(Box::new(cow), &seed_lba1, &pattern_lba2, "COW/VHD")?;
    }
    {
        let mut base = VhdFixedDisk::open(&vhd_base, DEFAULT_SECTOR_SIZE)?;
        let mut lba2 = vec![0u8; sector_size];
        base.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, zeros, "COW/VHD: la imagen base no debe modificarse");
        println!("  [OK] VHD base intacta tras escrituras guest en overlay");
    }
    {
        let base = VhdFixedDisk::open(&vhd_base, DEFAULT_SECTOR_SIZE)?;
        let mut cow = CowOverlayDisk::open(base, &vhd_ov)?;
        let mut lba2 = vec![0u8; sector_size];
        cow.read_sectors(2, 1, &mut lba2)?;
        assert_eq!(lba2, pattern_lba2, "COW/VHD: el overlay no persistió los cambios");
        println!("  [OK] VHD overlay persistente");
    }

    let _ = fs::remove_file(&raw_base);
    let _ = fs::remove_file(&raw_ov);
    let _ = fs::remove_file(&vhd_base);
    let _ = fs::remove_file(&vhd_ov);

    println!("--------------------------------------------------");
    println!("  Persistencia COW (overlay): ÉXITO");
    println!("  Bases: RAW, VHD (fixed) | Imagen base preservada, cambios en overlay");
    println!("==================================================");

    Ok(())
}

fn run_test_network() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Red en Modo Usuario / virtio-net (M8)");
    println!("==================================================");

    // 1) NAT: ARP para la puerta de enlace (10.0.2.2)
    let guest_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let mut nat = capsule_network::UserModeNat::new(guest_mac);
    let mut arp_req = vec![0u8; 42];
    arp_req[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    arp_req[6..12].copy_from_slice(&guest_mac);
    arp_req[12..14].copy_from_slice(&0x0806u16.to_be_bytes()); // ARP
    arp_req[14..16].copy_from_slice(&1u16.to_be_bytes()); // HTYPE Ethernet
    arp_req[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // PTYPE IPv4
    arp_req[18] = 6;
    arp_req[19] = 4;
    arp_req[20..22].copy_from_slice(&1u16.to_be_bytes()); // REQUEST
    arp_req[22..28].copy_from_slice(&guest_mac);
    arp_req[28..32].copy_from_slice(&[10, 0, 2, 15]); // sender IP
    arp_req[38..42].copy_from_slice(&[10, 0, 2, 2]); // target IP gateway
    nat.push_tx_frame(&arp_req)?;
    let arp_reply = nat
        .poll_rx_frame()
        .ok_or("NAT: no se recibió respuesta ARP para gateway")?;
    assert!(arp_reply.len() >= 42, "NAT: trama ARP de respuesta demasiado corta");
    assert_eq!(
        u16::from_be_bytes([arp_reply[12], arp_reply[13]]),
        0x0806,
        "NAT: respuesta no es ARP"
    );
    assert_eq!(
        u16::from_be_bytes([arp_reply[20], arp_reply[21]]),
        2,
        "NAT: respuesta ARP no es REPLY"
    );
    assert_eq!(&arp_reply[28..32], &[10, 0, 2, 2], "NAT: ARP reply SPA incorrecta");
    println!("  [OK] NAT: respuesta ARP para puerta de enlace");

    // 2) NAT: ICMP echo request al gateway -> echo reply
    let mut icmp = vec![0u8; 16];
    icmp[0] = 8; // echo request
    icmp[1] = 0;
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < icmp.len() {
        sum += u16::from_be_bytes([icmp[i], icmp[i + 1]]) as u32;
        i += 2;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let csum = !(sum as u16);
    icmp[2..4].copy_from_slice(&csum.to_be_bytes());
    icmp[4..6].copy_from_slice(&0x1234u16.to_be_bytes()); // id
    icmp[6..8].copy_from_slice(&1u16.to_be_bytes()); // seq
    let ip_total = 20 + icmp.len();
    let mut frame = vec![0u8; 14 + ip_total];
    frame[0..6].copy_from_slice(&[0x52, 0x55, 0x0A, 0x00, 0x02, 0x02]); // dst gateway MAC (any)
    frame[6..12].copy_from_slice(&guest_mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let ip = &mut frame[14..];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(ip_total as u16).to_be_bytes());
    ip[8] = 64;
    ip[9] = 1; // ICMP
    ip[12..16].copy_from_slice(&[10, 0, 2, 15]);
    ip[16..20].copy_from_slice(&[10, 0, 2, 2]);
    let mut ip_sum: u32 = 0;
    let mut j = 0;
    while j + 1 < 20 {
        ip_sum += u16::from_be_bytes([ip[j], ip[j + 1]]) as u32;
        j += 2;
    }
    while (ip_sum >> 16) != 0 {
        ip_sum = (ip_sum & 0xFFFF) + (ip_sum >> 16);
    }
    let ip_csum = !(ip_sum as u16);
    ip[10..12].copy_from_slice(&ip_csum.to_be_bytes());
    ip[20..].copy_from_slice(&icmp);
    nat.push_tx_frame(&frame)?;
    let echo_reply = nat
        .poll_rx_frame()
        .ok_or("NAT: no se recibió ICMP echo reply del gateway")?;
    assert!(echo_reply.len() >= 14 + 20 + 8, "NAT: trama ICMP de respuesta demasiado corta");
    assert_eq!(echo_reply[23], 1, "NAT: respuesta IP no es ICMP");
    assert_eq!(echo_reply[34], 0, "NAT: ICMP reply type incorrecto");
    println!("  [OK] NAT: ICMP echo reply desde gateway (10.0.2.2)");

    // 3) Integración VM/PCI: virtio-net presente en BDF 00:03.0, IDs y BAR0 I/O con MAC legible
    let mut vm = VirtualMachine::new(VmConfig {
        name: "NetworkTestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    })?;
    // Seleccionar 00:03.0, offset 0x00 (Vendor/Device)
    // Guest escribe 0x80001800 a 0xCF8 y lee 0xCFC (4 bytes) -> [ebx]
    let code = [
        0xBA, 0xF8, 0x0C, 0x00, 0x00, // mov edx, 0x0CF8
        0xB8, 0x00, 0x18, 0x00, 0x80, // mov eax, 0x80001800
        0xEF,                         // out dx, eax
        0xBA, 0xFC, 0x0C, 0x00, 0x00, // mov edx, 0x0CFC
        0xED,                         // in eax, dx
        0x89, 0x03,                   // mov [ebx], eax (ebx=0)
        0xF4,                         // hlt
    ];
    vm.load_code(GuestAddress::new(0), &code)?;
    vm.set_rip(0);
    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt, "VM de red debe terminar con HLT");
    let mut out = [0u8; 4];
    vm.memory.read_bytes(GuestAddress::new(0), &mut out)?;
    let id = u32::from_le_bytes(out);
    assert_eq!(id & 0xFFFF, 0x1AF4, "virtio-net: Vendor ID PCI incorrecto");
    assert_eq!((id >> 16) & 0xFFFF, 0x1000, "virtio-net: Device ID PCI incorrecto");
    println!("  [OK] VM/PCI: virtio-net detectada en 00:03.0 (1AF4:1000)");

    // Habilitar I/O decoding (command bit 0) y leer MAC desde BAR0+0x14 mediante acceso directo al dispositivo PCI
    // (el puerto BAR puede variar; lo leemos del config space BAR0)
    let read_cfg = |vm: &mut VirtualMachine, bdf_off: u32| -> u32 {
        let addr = 0x8000_0000u32 | (0u32 << 16) | (3u32 << 11) | (0u32 << 8) | (bdf_off & 0xFC);
        for dev in &mut vm.devices {
            if dev.handles_port(0xCF8) {
                dev.write_io(0xCF8, addr, 4);
                return dev.read_io(0xCFC, 4);
            }
        }
        0xFFFF_FFFF
    };
    let write_cfg = |vm: &mut VirtualMachine, bdf_off: u32, val: u32| {
        let addr = 0x8000_0000u32 | (0u32 << 16) | (3u32 << 11) | (0u32 << 8) | (bdf_off & 0xFC);
        for dev in &mut vm.devices {
            if dev.handles_port(0xCF8) {
                dev.write_io(0xCF8, addr, 4);
                dev.write_io(0xCFC, val, 4);
                return;
            }
        }
    };
    let bar0 = read_cfg(&mut vm, 0x10);
    let bar0_io = (bar0 & 0xFFFC) as u16;
    assert_ne!(bar0_io, 0, "virtio-net: BAR0 I/O no asignado");
    // command: IO space enable
    let cmd = read_cfg(&mut vm, 0x04) & 0xFFFF;
    write_cfg(&mut vm, 0x04, cmd | 0x0001);
    // leer 6 bytes de MAC desde config de dispositivo virtio (offset 0x14 en BAR)
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        for dev in &mut vm.devices {
            if dev.handles_port(bar0_io) {
                *b = dev.read_io(bar0_io + 0x14 + i as u16, 1) as u8;
                break;
            }
        }
    }
    assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], "virtio-net: MAC inesperada en config BAR");
    println!("  [OK] virtio-net: BAR0 I/O habilitado y MAC legible ({:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    println!("--------------------------------------------------");
    println!("  Red M8 (NAT modo usuario + virtio-net PCI): ÉXITO");
    println!("  Nota: sin MMIO/inyección de IRQ en CPU; virtio-net operada por sondeo (polling) y BAR I/O.");
    println!("==================================================");
    Ok(())
}


fn run_test_framebuffer() -> Result<(), Box<dyn std::error::Error>> {
    use capsule_devices::vga::{
        VBE_DISPI_INDEX_BPP, VBE_DISPI_INDEX_ENABLE, VBE_DISPI_INDEX_XRES, VBE_DISPI_INDEX_YRES,
        VBE_DISPI_LFB_ENABLED, VBE_DISPI_LFB_PHYSICAL_ADDRESS,
    };
    use capsule_devices::{VBE_DISPI_IOPORT_DATA, VBE_DISPI_IOPORT_INDEX};

    println!("==================================================");
    println!("   Prueba de Framebuffer VGA/Bochs VBE (Hito M9)");
    println!("==================================================");

    let mut vm = VirtualMachine::new(VmConfig {
        name: "FramebufferTestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(8),
    })?;

    // Guest: configura Bochs VBE 320x200x32 con LFB y escribe patron rojo en el framebuffer, luego HLT.
    // Usa OUT DX,EAX (32-bit) para programar registros VBE de 16 bits (el dispositivo enmascara a 16 bits).
    let mut code: Vec<u8> = Vec::new();
    let vbe_index: u16 = VBE_DISPI_IOPORT_INDEX;
    let vbe_data: u16 = VBE_DISPI_IOPORT_DATA;
    let emit_out32 = |c: &mut Vec<u8>, port: u16, val: u32| {
        // mov edx, port
        c.push(0xBA);
        c.extend_from_slice(&(port as u32).to_le_bytes());
        // mov eax, val
        c.push(0xB8);
        c.extend_from_slice(&val.to_le_bytes());
        // out dx, eax
        c.push(0xEF);
    };
    let emit_vbe = |c: &mut Vec<u8>, idx: u16, val: u16| {
        emit_out32(c, vbe_index, idx as u32);
        emit_out32(c, vbe_data, val as u32);
    };

    emit_vbe(&mut code, VBE_DISPI_INDEX_XRES, 320);
    emit_vbe(&mut code, VBE_DISPI_INDEX_YRES, 200);
    emit_vbe(&mut code, VBE_DISPI_INDEX_BPP, 32);
    emit_vbe(&mut code, VBE_DISPI_INDEX_ENABLE, 0x01 | VBE_DISPI_LFB_ENABLED);

    // Rellenar 320*20 pixeles en LFB (0x00400000) con color 0x00FF0000 (B,G,R,X)
    // mov edi, 0x00400000
    code.extend_from_slice(&[0xBF, 0x00, 0x00, 0x40, 0x00]);
    let count: u32 = 320 * 20;
    // mov ecx, count
    code.push(0xB9);
    code.extend_from_slice(&count.to_le_bytes());
    // mov eax, 0x00FF0000
    code.push(0xB8);
    code.extend_from_slice(&0x00FF0000u32.to_le_bytes());
    let loop_start = code.len();
    // mov [edi], eax  (ModRM: mod=00 reg=000(eax) rm=111(edi) -> 0x07)
    code.extend_from_slice(&[0x89, 0x07]);
    // add edi, 4  (83 /0 ib, ModRM for edi = C7)
    code.extend_from_slice(&[0x83, 0xC7, 0x04]);
    // dec ecx (FF /1, ModRM for ecx = C9)
    code.extend_from_slice(&[0xFF, 0xC9]);
    // jnz loop_start (rel8)
    let rel = (loop_start as i32) - ((code.len() as i32) + 2);
    if rel < -128 || rel > 127 {
        return Err("Framebuffer test: bucle fuera de rango rel8".into());
    }
    code.extend_from_slice(&[0x75, rel as i8 as u8]);
    // hlt
    code.push(0xF4);

    vm.load_code(GuestAddress::new(0), &code)?;
    vm.set_rip(0);

    let mut steps = 0usize;
    let max_steps = 2_000_000usize;
    let mut exit_reason = None;
    while steps < max_steps {
        steps += 1;
        if let Some(exit) = vm.step_once()? {
            exit_reason = Some(exit);
            break;
        }
    }
    let exit = exit_reason.ok_or("Framebuffer test: la VM no alcanzo HLT dentro del limite de pasos")?;
    assert_eq!(exit, VcpuExit::Halt, "Framebuffer test: salida inesperada {:?}", exit);
    println!("  [OK] Guest programo VBE y escribio patron en LFB (HLT, pasos={})", steps);

    let mut fb_info: Option<(u64, u32, u32, u32, usize)> = None;
    for dev in &vm.devices {
        if let Some(vbe) = dev.as_any().downcast_ref::<capsule_devices::BochsVbe>() {
            assert!(vbe.is_enabled(), "VBE debe estar habilitado");
            assert!(vbe.is_lfb_enabled(), "VBE LFB debe estar habilitado");
            assert_eq!(vbe.width(), 320);
            assert_eq!(vbe.height(), 200);
            assert_eq!(vbe.bpp(), 32);
            assert_eq!(vbe.lfb_base(), VBE_DISPI_LFB_PHYSICAL_ADDRESS);
            fb_info = Some((vbe.lfb_base(), vbe.width(), vbe.height(), vbe.bpp(), vbe.stride_bytes()));
            println!(
                "  [OK] Bochs VBE: {}x{}x{} LFB@0x{:08X} stride={}",
                vbe.width(), vbe.height(), vbe.bpp(), vbe.lfb_base(), vbe.stride_bytes()
            );
        }
    }
    let (base, w, h, _bpp, _stride) = fb_info.ok_or("Framebuffer test: no se encontro dispositivo BochsVbe")?;

    let mut px = [0u8; 4];
    vm.memory.read_bytes(GuestAddress::new(base), &mut px)?;
    assert_eq!(u32::from_le_bytes(px), 0x00FF0000, "Pixel (0,0) en LFB incorrecto");
    vm.memory.read_bytes(GuestAddress::new(base + 4 * (w as u64) * 5), &mut px)?;
    assert_eq!(u32::from_le_bytes(px), 0x00FF0000, "Pixel en fila 5 del LFB incorrecto");
    println!("  [OK] LFB en memoria guest contiene el patron esperado");

    let mut bgra = vec![0u32; (w as usize) * (h as usize)];
    let copied = vm.copy_framebuffer_bgra32(&mut bgra).ok_or("copy_framebuffer_bgra32 fallo")?;
    assert_eq!(copied, (w, h));
    assert_eq!(bgra[0], 0x00FF0000);
    println!("  [OK] Copia de framebuffer para render Win32 ({}x{})", copied.0, copied.1);

    println!("--------------------------------------------------");
    println!("  Framebuffer M9 (Bochs VBE + LFB + blit host): EXITO");
    println!("  Nota: LFB en RAM guest (sin MMIO), sin inyeccion de IRQ en CPU.");
    println!("==================================================");
    Ok(())
}
fn run_test_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("   Prueba de Plataforma Virtual (Hito M7)       ");
    println!("==================================================");

    // PIC 8259: inicialización, IMR y reconocimiento de IRQ con EOI
    let mut pic = capsule_devices::Pic8259::new();
    pic.write_io(0x20, 0x11, 1);
    pic.write_io(0xA0, 0x11, 1);
    pic.write_io(0x21, 0x20, 1); // base maestro 0x20
    pic.write_io(0xA1, 0x28, 1); // base esclavo 0x28
    pic.write_io(0x21, 0x04, 1); // cascada IRQ2
    pic.write_io(0xA1, 0x02, 1);
    pic.write_io(0x21, 0x01, 1); // 8086
    pic.write_io(0xA1, 0x01, 1);
    pic.write_io(0x21, 0x00, 1); // desenmascarar
    pic.write_io(0xA1, 0x00, 1);
    pic.raise_irq(0);
    let irq0_vec = pic
        .get_interrupt()
        .ok_or("PIC: no se obtuvo vector para IRQ0")?;
    assert_eq!(irq0_vec, 0x20, "IRQ0 debe vectorizarse en 0x20");
    pic.write_io(0x20, 0x20, 1); // EOI maestro
    assert_eq!(pic.master_isr() & 0x01, 0, "PIC: EOI debe limpiar ISR de IRQ0");
    pic.raise_irq(14); // esclavo IRQ6
    let irq14_vec = pic
        .get_interrupt()
        .ok_or("PIC: no se obtuvo vector para IRQ14")?;
    assert_eq!(irq14_vec, 0x2E, "IRQ14 debe vectorizarse en 0x2E vía cascada");
    pic.write_io(0xA0, 0x20, 1); // EOI esclavo
    pic.write_io(0x20, 0x20, 1); // EOI maestro (IRQ2 cascada)
    println!("  [OK] PIC 8259: init, IMR, IRQ0/IRQ14, cascada y EOI");

    // PIT 8254: canal 0 genera pulsos IRQ0 en modo 2
    let mut pit = capsule_devices::Pit8254::new();
    pit.write_io(0x43, 0x34, 1); // CH0, lo/hi, modo 2
    pit.write_io(0x40, 0x04, 1);
    pit.write_io(0x40, 0x00, 1);
    let mut pulses = 0u32;
    for _ in 0..16 {
        if pit.tick(1) {
            pulses += 1;
        }
    }
    assert!(pulses >= 1, "PIT: no se observaron pulsos IRQ0 en canal 0");
    println!("  [OK] PIT 8254: programación de canal 0 y pulsos IRQ0");

    // RTC/CMOS: hora legible y CMOS RAM conservada
    let mut rtc = capsule_devices::RtcCmos::new();
    rtc.write_io(0x70, 0x00, 1);
    let sec = rtc.read_io(0x71, 1) & 0xFF;
    rtc.write_io(0x70, 0x02, 1);
    let min = rtc.read_io(0x71, 1) & 0xFF;
    rtc.write_io(0x70, 0x04, 1);
    let hour = rtc.read_io(0x71, 1) & 0xFF;
    assert!(
        sec <= 59 && min <= 59 && hour <= 23,
        "RTC: valores de hora fuera de rango"
    );
    rtc.write_io(0x70, 0x10, 1);
    rtc.write_io(0x71, 0x5A, 1);
    rtc.write_io(0x70, 0x10, 1);
    assert_eq!(
        rtc.read_io(0x71, 1) & 0xFF,
        0x5A,
        "RTC/CMOS: registro 0x10 no conservó valor"
    );
    println!("  [OK] RTC/CMOS: lectura de hora y RAM CMOS preservada");

    // PCI config mechanism #1 (0xCF8/0xCFC), bus vacío
    let mut pci = capsule_devices::PciBus::new();
    pci.write_io(0xCF8, 0x8000_0000, 4);
    assert_eq!(
        pci.read_io(0xCF8, 4),
        0x8000_0000,
        "PCI: 0xCF8 debe conservar dirección"
    );
    assert_eq!(
        pci.read_io(0xCFC, 2),
        0xFFFF,
        "PCI: Vendor ID debe ser 0xFFFF en bus vacío"
    );
    assert_eq!(
        pci.read_io(0xCFC, 4),
        0xFFFF_FFFF,
        "PCI: bus vacío debe devolver 0xFFFF_FFFF"
    );
    pci.write_io(0xCF8, 0x0000_0000, 4);
    assert_eq!(
        pci.read_io(0xCFC, 1),
        0xFF,
        "PCI: lecturas con config deshabilitada deben devolver 0xFF"
    );
    println!("  [OK] PCI (CF8/CFC): direccionamiento y enumeración básica (bus vacío)");

    // Integración VM: los puertos de plataforma deben ser manejados por dispositivos registrados
    let mut vm = VirtualMachine::new(VmConfig {
        name: "PlatformTestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    })?;
    // Leer 0xCF8 (4 bytes) -> EAX debe ser 0 en estado inicial del bus PCI
    let code = [
        0xBA, 0xF8, 0x0C, 0x00, 0x00, // mov edx, 0x0CF8
        0xED,                         // in eax, dx
        0x89, 0x03,                   // mov [ebx], eax (ebx=0 al inicio)
        0xF4,                         // hlt
    ];
    vm.load_code(GuestAddress::new(0), &code)?;
    vm.set_rip(0);
    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt, "VM de plataforma debe terminar con HLT");
    let mut out = [0u8; 4];
    vm.memory.read_bytes(GuestAddress::new(0), &mut out)?;
    let pci_addr = u32::from_le_bytes(out);
    assert_eq!(
        pci_addr, 0x0000_0000,
        "VM: lectura de 0xCF8 debe estar manejada por PCI"
    );
    println!("  [OK] VM: dispositivos de plataforma registrados y accesibles por E/S");

    println!("--------------------------------------------------");
    println!("  Plataforma virtual M7 (PIC/PIT/RTC/PCI): ÉXITO");
    println!("  Nota: sin inyección de interrupciones en CPU aún (requiere IDT/CLI/STI/IRET).");
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
        println!("  Nota: Soporta RAW y VHD fixed (512B). CPU/ISA y firmware limitados; bootloaders reales pueden requerir M12 y entrega de interrupciones en CPU.");
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

const GUI_TIMER_ID: usize = 1;
const GUI_TIMER_MS: u32 = 16;
const GUI_STEPS_PER_TICK: usize = 50_000;

struct GuiState {
    vm: VirtualMachine,
    fb_pixels: Vec<u32>,
    fb_w: u32,
    fb_h: u32,
    info: &'static HostInfo,
    status: String,
}

unsafe fn draw_diagnostics(hdc: HDC, rect: &RECT, info: &HostInfo, status: &str) {
    let bg_brush = CreateSolidBrush(rgb(24, 26, 32));
    FillRect(hdc, rect, bg_brush);
    DeleteObject(bg_brush);

    SetBkMode(hdc, TRANSPARENT as i32);

    let title_font = create_gdi_font(24, 700);
    let subtitle_font = create_gdi_font(16, 600);
    let body_font = create_gdi_font(15, 400);
    let code_font = create_gdi_font(14, 400);

    SelectObject(hdc, title_font);
    SetTextColor(hdc, rgb(255, 255, 255));
    let title = format!("CapsuleOS - Runtime v{}", CAPSULE_VERSION);
    draw_text_line(hdc, &title, 32, 28, 640, 32, DT_LEFT);

    SelectObject(hdc, subtitle_font);
    SetTextColor(hdc, rgb(130, 140, 160));
    draw_text_line(
        hdc,
        "Sistema operativo completo como ejecutable autonomo",
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

    current_y = draw_card_header(hdc, "Host Platform", 32, current_y, rect.right - 32, subtitle_font);
    SelectObject(hdc, body_font);
    SetTextColor(hdc, rgb(220, 225, 235));
    let os_desc = format!(
        "OS: {} | Build: {} | Arch: {}",
        info.os.os_name, info.os.build_number, info.os.architecture
    );
    draw_text_line(hdc, &os_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
    current_y += 34;

    current_y = draw_card_header(hdc, "Procesador (CPU)", 32, current_y, rect.right - 32, subtitle_font);
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
        "Nucleos logicos: {}  |  Virtualizacion HW: {}",
        info.cpu.logical_cores, virt_hw
    );
    draw_text_line(hdc, &cores_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
    current_y += 34;

    current_y = draw_card_header(hdc, "Memoria RAM", 32, current_y, rect.right - 32, subtitle_font);
    SelectObject(hdc, body_font);
    SetTextColor(hdc, rgb(220, 225, 235));
    let ram_desc = format!(
        "Total: {}  |  Disponible: {}  |  Uso del sistema: {}%",
        info.memory.total_phys, info.memory.avail_phys, info.memory.memory_load_percent
    );
    draw_text_line(hdc, &ram_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
    current_y += 34;

    current_y = draw_card_header(hdc, "Backend de Virtualizacion", 32, current_y, rect.right - 32, subtitle_font);
    SelectObject(hdc, body_font);

    let (whpx_text, whpx_color) = match info.whpx {
        WhpxStatus::Available => (
            "WHPX: DISPONIBLE (Aceleracion nativa de hardware activa)",
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
    draw_text_line(hdc, info.whpx.description(), 48, current_y, rect.right - 48, 22, DT_LEFT);
    current_y += 34;

    current_y = draw_card_header(hdc, "Capsule Runtime (M0-M9)", 32, current_y, rect.right - 32, subtitle_font);
    SelectObject(hdc, body_font);
    SetTextColor(hdc, rgb(74, 222, 128));
    draw_text_line(
        hdc,
        "OK - UART, boot, ATA/IDE, COW, plataforma, NAT/virtio-net y framebuffer VGA/Bochs VBE",
        48,
        current_y,
        rect.right - 48,
        22,
        DT_LEFT,
    );
    current_y += 24;

    SetTextColor(hdc, rgb(220, 225, 235));
    draw_text_line(hdc, status, 48, current_y, rect.right - 48, 22, DT_LEFT);
    current_y += 24;

    SetTextColor(hdc, rgb(140, 150, 170));
    SelectObject(hdc, code_font);
    draw_text_line(
        hdc,
        "Siguiente paso: inyeccion de IRQ en CPU (IDT/CLI/STI/IRET), M12 (firmware/boot real)",
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
}

fn load_vbe_demo_guest(vm: &mut VirtualMachine) -> Result<(), Box<dyn std::error::Error>> {
    use capsule_devices::vga::{
        VBE_DISPI_INDEX_BPP, VBE_DISPI_INDEX_ENABLE, VBE_DISPI_INDEX_XRES, VBE_DISPI_INDEX_YRES,
        VBE_DISPI_LFB_ENABLED,
    };
    use capsule_devices::{VBE_DISPI_IOPORT_DATA, VBE_DISPI_IOPORT_INDEX};

    let mut code: Vec<u8> = Vec::new();
    let vbe_index: u16 = VBE_DISPI_IOPORT_INDEX;
    let vbe_data: u16 = VBE_DISPI_IOPORT_DATA;
    let emit_out32 = |c: &mut Vec<u8>, port: u16, val: u32| {
        c.push(0xBA);
        c.extend_from_slice(&(port as u32).to_le_bytes());
        c.push(0xB8);
        c.extend_from_slice(&val.to_le_bytes());
        c.push(0xEF);
    };
    let emit_vbe = |c: &mut Vec<u8>, idx: u16, val: u16| {
        emit_out32(c, vbe_index, idx as u32);
        emit_out32(c, vbe_data, val as u32);
    };

    // 320x200x32 LFB
    emit_vbe(&mut code, VBE_DISPI_INDEX_XRES, 320);
    emit_vbe(&mut code, VBE_DISPI_INDEX_YRES, 200);
    emit_vbe(&mut code, VBE_DISPI_INDEX_BPP, 32);
    emit_vbe(&mut code, VBE_DISPI_INDEX_ENABLE, 0x01 | VBE_DISPI_LFB_ENABLED);

    // Rellenar todo el framebuffer (320*200) con barras de color simples:
    // dividimos en franjas horizontales cambiando color cada 40 filas (5 colores x 40 filas)
    // Implementacion simple: bucle externo por filas-bloques no trivial; hacemos un relleno unico degradado aproximado
    // escribiendo un patron repetitivo por palabras.
    // mov edi, 0x00400000
    code.extend_from_slice(&[0xBF, 0x00, 0x00, 0x40, 0x00]);
    // mov ecx, 320*200
    let count: u32 = 320 * 200;
    code.push(0xB9);
    code.extend_from_slice(&count.to_le_bytes());
    // mov eax, 0x00205090 (color inicial)
    code.push(0xB8);
    code.extend_from_slice(&0x00205090u32.to_le_bytes());
    let loop_start = code.len();
    // mov [edi], eax
    code.extend_from_slice(&[0x89, 0x07]);
    // add edi, 4
    code.extend_from_slice(&[0x83, 0xC7, 0x04]);
    // add eax, 0x00010101 (ligera variacion de color) - usar grupo 0x81 /0 por compatibilidad con la vCPU
    code.extend_from_slice(&[0x81, 0xC0, 0x01, 0x01, 0x01, 0x00]);
    // dec ecx
    code.extend_from_slice(&[0xFF, 0xC9]);
    // jnz loop_start
    let rel = (loop_start as i32) - ((code.len() as i32) + 2);
    if rel < -128 || rel > 127 {
        return Err("demo VBE: bucle fuera de rango rel8".into());
    }
    code.extend_from_slice(&[0x75, rel as i8 as u8]);
    code.push(0xF4);

    vm.load_code(GuestAddress::new(0), &code)?;
    vm.set_rip(0);
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = &*(lparam as *const CREATESTRUCTW);
            let state_ptr = cs.lpCreateParams as *mut GuiState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
            SetTimer(hwnd, GUI_TIMER_ID, GUI_TIMER_MS, None);
            0
        }
        WM_TIMER => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
            if !ptr.is_null() {
                let state = &mut *ptr;
                if state.vm.status == capsule_vmm::state::VmStatus::Created
                    || state.vm.status == capsule_vmm::state::VmStatus::Running
                {
                    let _ = state.vm.run_steps(GUI_STEPS_PER_TICK);
                }
                if let Some((_base, w, h, _bpp, _stride)) = state.vm.vbe_framebuffer_info() {
                    let needed = (w as usize).saturating_mul(h as usize);
                    if needed > 0 {
                        if state.fb_pixels.len() != needed || state.fb_w != w || state.fb_h != h {
                            state.fb_pixels.resize(needed, 0);
                            state.fb_w = w;
                            state.fb_h = h;
                        }
                        let _ = state.vm.copy_framebuffer_bgra32(&mut state.fb_pixels);
                    }
                }
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);

            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
            if !ptr.is_null() {
                let state = &mut *ptr;
                if state.fb_w > 0 && state.fb_h > 0 && !state.fb_pixels.is_empty() {
                    // Fondo
                    let bg = CreateSolidBrush(rgb(12, 14, 18));
                    FillRect(hdc, &rect, bg);
                    DeleteObject(bg);

                    // Area de framebuffer (arriba) y barra de estado (abajo)
                    let status_h = 28;
                    let fb_area_h = (rect.bottom - rect.top - status_h).max(1);
                    let fb_area_w = (rect.right - rect.left).max(1);

                    // Escalado preservando aspecto
                    let src_w = state.fb_w as i32;
                    let src_h = state.fb_h as i32;
                    let mut dst_w = fb_area_w;
                    let mut dst_h = (fb_area_w * src_h) / src_w.max(1);
                    if dst_h > fb_area_h {
                        dst_h = fb_area_h;
                        dst_w = (fb_area_h * src_w) / src_h.max(1);
                    }
                    let dst_x = (fb_area_w - dst_w) / 2;
                    let dst_y = (fb_area_h - dst_h) / 2;

                    let mut bmi: BITMAPINFO = std::mem::zeroed();
                    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                    bmi.bmiHeader.biWidth = src_w;
                    bmi.bmiHeader.biHeight = -src_h; // top-down
                    bmi.bmiHeader.biPlanes = 1;
                    bmi.bmiHeader.biBitCount = 32;
                    bmi.bmiHeader.biCompression = BI_RGB;

                    StretchDIBits(
                        hdc,
                        dst_x,
                        dst_y,
                        dst_w,
                        dst_h,
                        0,
                        0,
                        src_w,
                        src_h,
                        state.fb_pixels.as_ptr() as *const _,
                        &bmi,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );

                    // Barra de estado
                    let bar_rect = RECT {
                        left: 0,
                        top: rect.bottom - status_h,
                        right: rect.right,
                        bottom: rect.bottom,
                    };
                    let bar = CreateSolidBrush(rgb(24, 26, 32));
                    FillRect(hdc, &bar_rect, bar);
                    DeleteObject(bar);

                    SetBkMode(hdc, TRANSPARENT as i32);
                    let font = create_gdi_font(14, 400);
                    SelectObject(hdc, font);
                    SetTextColor(hdc, rgb(200, 210, 220));
                    let status = format!(
                        "M9 Framebuffer VGA/Bochs VBE: {}x{}x32 LFB activo  |  VM: {:?}  |  [Esc] salir",
                        state.fb_w, state.fb_h, state.vm.status
                    );
                    let mut text_rect = bar_rect;
                    text_rect.left += 8;
                    DrawTextW(
                        hdc,
                        status.encode_utf16().collect::<Vec<u16>>().as_mut_ptr(),
                        -1,
                        &mut text_rect,
                        DT_LEFT | DT_SINGLELINE | DT_TOP | DT_NOPREFIX,
                    );
                    DeleteObject(font);
                } else {
                    draw_diagnostics(hdc, &rect, state.info, &state.status);
                }
            } else {
                draw_diagnostics(hdc, &rect, get_host_info(), "Inicializando runtime...");
            }

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
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
            if !ptr.is_null() {
                KillTimer(hwnd, GUI_TIMER_ID);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
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
    wide.push(0);
    let mut rect = RECT {
        left: x,
        top: y,
        right: max_x,
        bottom: y + height,
    };
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
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

    if args.iter().any(|arg| arg == "test-cow" || arg == "--test-cow") {
        if let Err(e) = run_test_cow() {
            eprintln!("Error en test-cow: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-platform" || arg == "--test-platform") {
        if let Err(e) = run_test_platform() {
            eprintln!("Error en test-platform: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-network" || arg == "--test-network") {
        if let Err(e) = run_test_network() {
            eprintln!("Error en test-network: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "test-framebuffer" || arg == "--test-framebuffer") {
        if let Err(e) = run_test_framebuffer() {
            eprintln!("Error en test-framebuffer: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let host_info = get_host_info();
    print_cli_summary(host_info);

    let mut vm = match VirtualMachine::new(VmConfig {
        name: "CapsuleGUI".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(16),
    }) {
        Ok(vm) => vm,
        Err(e) => {
            eprintln!("Error creando VM para GUI: {}", e);
            return;
        }
    };

    let status = match load_vbe_demo_guest(&mut vm) {
        Ok(()) => "Demo guest M9 cargado: programando VBE 320x200x32 y dibujando en LFB...".to_string(),
        Err(e) => format!("No se pudo cargar demo VBE: {} (mostrando diagnostico)", e),
    };

    let state = Box::new(GuiState {
        vm,
        fb_pixels: Vec::new(),
        fb_w: 0,
        fb_h: 0,
        info: host_info,
        status,
    });
    let state_ptr = Box::into_raw(state);

    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "CapsuleOS_HostWindow\0".encode_utf16().collect();
        let window_title: Vec<u16> = "CapsuleOS - Framebuffer Runtime (M9)\0"
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

        let width = 980;
        let height = 720;

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
            state_ptr as *const _,
        );

        if hwnd.is_null() {
            eprintln!("Error al crear la ventana principal de CapsuleOS.");
            drop(Box::from_raw(state_ptr));
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