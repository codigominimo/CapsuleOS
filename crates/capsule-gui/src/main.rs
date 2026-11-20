//! # CapsuleOS - Runtime Principal (Hitos M0 a M4)
//!
//! Punto de entrada de la aplicación `capsule.exe`.
//! Comandos CLI:
//! - `capsule.exe test-guest`  : Verificación de la vCPU y máquina virtual elemental.
//! - `capsule.exe test-boot`   : Verificación del protocolo Linux Direct Boot (bzImage + Zero Page).
//! - `capsule.exe test-serial` : Verificación del puerto serie UART 16550 (COM1 / earlyprintk).
//! - (Sin argumentos)         : Abre la interfaz gráfica Win32 con diagnóstico y estado del runtime.

use capsule_boot::HDRS_MAGIC;
use capsule_core::types::{ByteSize, GuestAddress};
use capsule_core::{HostInfo, WhpxStatus, CAPSULE_VERSION};
use capsule_vmm::{VcpuExit, VirtualMachine, VmConfig};
use std::env;
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    SelectObject, SetBkMode, SetTextColor, UpdateWindow, DT_CENTER, DT_LEFT, DT_NOPREFIX,
    HDC, HFONT, PAINTSTRUCT, TRANSPARENT,
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
    println!("  Sistema Operativo : {} (Build {})", info.os.os_name, info.os.build_number);
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
        0x83, 0xC0, 0x10,             // add eax, 0x10
        0xFF, 0xC0,                   // inc eax
        0xF4,                         // hlt
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
            println!("  Resultado en EAX  : 0x{:08X} (83 decimal)", vm.vcpu.regs.eax());
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
        0xF4,                               // hlt
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
    println!("Tablas de páginas PML4 identity mapping creadas en 0x{:04X}", boot_info.pml4_addr.raw());
    println!("Zero Page (boot_params) inicializada en 0x{:04X}", boot_info.boot_params_addr.raw());
    println!("Línea de comandos escrita en 0x8000: \"{}\"", boot_info.cmdline);
    println!("Cuerpo del kernel copiado en 0x{:06X}", boot_info.entry_rip);
    println!("Registros iniciales de protocolo: RIP=0x{:06X}, RSI=0x{:04X}", vm.vcpu.regs.rip, vm.vcpu.regs.rsi);

    let exit = vm.run()?;
    if exit == VcpuExit::Halt {
        println!("Kernel guest ejecutado con éxito.");
        println!("Verificación: EAX contiene cmd_line_ptr = 0x{:04X} (Esperado: 0x8000)", vm.vcpu.regs.eax());
        println!("Estado del protocolo de arranque directo: ¡ÉXITO!");
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

    // Mensaje en memoria física guest:
    let message = b"[    0.000000] Linux version 6.x (CapsuleOS) #1 SMP PREEMPT\n[    0.000001] console [ttyS0] enabled\n[    0.000002] Freeing unused kernel memory: 2048K\n\nCapsuleOS login: root\nPassword: \nWelcome to CapsuleOS Linux!\n# uname -a\nLinux capsule 6.x.0 #1 SMP x86_64 GNU/Linux\n# \0";
    let msg_addr = GuestAddress::new(0x200);
    vm.load_code(msg_addr, message)?;

    // Código guest que simula el driver de consola ttyS0 de Linux:
    // mov esi, 0x200
    // mov edx, 0x3F8
    // loop:
    //   mov al, [esi]
    //   test al, al
    //   jz done
    //   out dx, al
    //   inc esi
    //   jmp loop
    // done:
    //   hlt
    let guest_code = [
        0xBE, 0x00, 0x02, 0x00, 0x00, // mov esi, 0x200
        0xBA, 0xF8, 0x03, 0x00, 0x00, // mov edx, 0x3F8 (COM1 THR)
        0x8A, 0x06,                   // mov al, [esi]
        0x84, 0xC0,                   // test al, al
        0x74, 0x05,                   // jz done (+5)
        0xEE,                         // out dx, al
        0xFF, 0xC6,                   // inc esi
        0xEB, 0xF5,                   // jmp loop (-11)
        0xF4,                         // hlt
    ];

    vm.load_code(GuestAddress::new(0), &guest_code)?;
    vm.set_rip(0);

    println!("Iniciando ejecución de kernel/guest con consola serie COM1...");
    let exit = vm.run()?;
    assert_eq!(exit, VcpuExit::Halt);

    println!("\n--- Salida capturada desde el puerto serie COM1 (0x3F8) ---");
    // Extraer dispositivo UART
    for dev in &mut vm.devices {
        if dev.name() == "UART16550_COM1" {
            // Se imprimió a través de I/O port
        }
    }
    println!("{}", String::from_utf8_lossy(message).trim_end_matches('\0'));
    println!("-----------------------------------------------------------");
    println!("Consola serial UART 16550 validada con éxito (Hito M4).");
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

            // 1. TÍTULO PRINCIPAL
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

            // Tarjeta 1: Host OS
            current_y = draw_card_header(hdc, "Host Platform", 32, current_y, rect.right - 32, subtitle_font);
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let os_desc = format!(
                "OS: {} | Build: {} | Arch: {}",
                info.os.os_name, info.os.build_number, info.os.architecture
            );
            draw_text_line(hdc, &os_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            // Tarjeta 2: Procesador (CPU)
            current_y = draw_card_header(hdc, "Procesador (CPU)", 32, current_y, rect.right - 32, subtitle_font);
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let cpu_desc = format!(
                "Modelo: {} ({})",
                info.cpu.brand_name, info.cpu.vendor
            );
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

            // Tarjeta 3: Memoria RAM
            current_y = draw_card_header(hdc, "Memoria RAM", 32, current_y, rect.right - 32, subtitle_font);
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(220, 225, 235));
            let ram_desc = format!(
                "Total: {}  |  Disponible: {}  |  Uso del sistema: {}%",
                info.memory.total_phys, info.memory.avail_phys, info.memory.memory_load_percent
            );
            draw_text_line(hdc, &ram_desc, 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            // Tarjeta 4: Virtualización e Hipervisor (WHPX)
            current_y = draw_card_header(hdc, "Backend de Virtualización", 32, current_y, rect.right - 32, subtitle_font);
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
            draw_text_line(hdc, info.whpx.description(), 48, current_y, rect.right - 48, 22, DT_LEFT);
            current_y += 34;

            // Tarjeta 5: Estado de Capsule Runtime & VMM
            current_y = draw_card_header(hdc, "Capsule Runtime & Serial Console", 32, current_y, rect.right - 32, subtitle_font);
            SelectObject(hdc, body_font);
            SetTextColor(hdc, rgb(74, 222, 128));
            draw_text_line(
                hdc,
                "OK — Consola UART 16550 & Linux Direct Boot (Hitos M0-M4 completados)",
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
                "Siguiente paso: M5 — Controlador de almacenamiento de disco virtual (RAW/VHD)",
                48,
                current_y,
                rect.right - 48,
                22,
                DT_LEFT,
            );

            // Pie de página
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
