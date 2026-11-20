# Hoja de Ruta (Roadmap) de CapsuleOS

Este documento detalla el plan de desarrollo paso a paso para el proyecto CapsuleOS. El desarrollo se realiza de forma estrictamente incremental (hito a hito), donde cada hito debe ser validado y probado antes de avanzar al siguiente.

---

## Resumen de Hitos

| Hito | Nombre | Objetivo Principal | Estado |
|---|---|---|---|
| **M0** | **Host Detection & Window** | Workspace Rust, ventana nativa Win32, detección de CPU, RAM y WHPX | **Completado** |
| **M1** | **Minimal VM Execution** | VMM elemental, asignación de memoria guest, ejecución de instrucciones simples y detección de HLT | **Completado** |
| **M2** | **CPU Emulator (x86_64)** | Intérprete x86_64 software: registros, decodificación básica, memoria y saltos | **Completado** |
| **M3** | **Direct Linux Boot** | Soporte del protocolo de arranque de Linux (bzImage direct boot) y carga de kernel | **Completado** |
| **M4** | **Linux Console & Shell** | Arranque de Linux con initramfs BusyBox y consola interactiva COM1/UART | **Completado** |
| **M5** | **Virtual Disk (RAW/VHD)** | Controlador ATA/IDE PIO, imágenes RAW/VHD fixed y arranque mínimo desde disco (LBA0) | **Completado** |
| **M6** | **Copy-on-Write Persistence** | Overlay COW (bitmap por sector) sobre imágenes base RAW/VHD sin modificarlas, con persistencia | **Completado** |
| **M7** | **Virtual Platform Devices** | PIC 8259 (dual, maestro/esclavo), PIT 8254, RTC/CMOS MC146818 y bus PCI extensible (config 0xCF8/0xCFC) integrados en la VM; IRQ0 (PIT) e IRQ14 (IDE primario, nivel) cableadas al PIC; validación con `test-platform` y pruebas unitarias | **Completado** |
| **M8** | **Networking (User-mode NAT)**| Virtio-net transitional PCI legacy (I/O BAR0, split virtqueues RX/TX) con NAT en modo usuario (ARP, DHCP básico, ICMP, UDP por sondeo) e IRQ INTx PCI agregada en PIC IRQ11; validación con `test-network` y pruebas | **Completado** |
| **M9** | **Framebuffer & Window Blit**| Adaptador gráfico VGA/Bochs VBE (puertos DISPI) con LFB en RAM guest, renderizado en tiempo real en ventana Win32 (DIB/StretchDIBits) y validación `test-framebuffer` | **Completado** |
| **M10**| **GUI Input & PS/2** | Controlador PS/2 i8042 (0x60/0x64) con teclado/ratÃ³n, IRQ1/IRQ12 reflejadas al PIC por sondeo, inyecciÃ³n de eventos Win32 desde GUI y validaciÃ³n `test-input` | **Completado** |
| **M11**| **WHPX Hardware Virtualization**| Backend de aceleracion por hardware con Windows Hypervisor Platform (WHPX), mapeo GPA y VM-exits de E/S hacia dispositivos emulados, con fallback a software | **Completado** |
| **M12**| **Firmware BIOS & UEFI** | Integración de SeaBIOS y OVMF para arranque estándar desde disco/ISO | Pendiente |
| **M13**| **Ubuntu Minimal / Server** | Arranque verificado de distribución Ubuntu Server mediante WHPX | Pendiente |
| **M14**| **Ubuntu Desktop (GUI)** | Arranque verificado de entorno gráfico X11/Wayland de Ubuntu en ventana | Pendiente |
| **M15**| **Capsule Bundle Format** | Especificación del contenedor monolitico: Header, Manifest, Payload comprimido | Pendiente |
| **M16**| **Capsule Builder CLI** | Herramienta `capsule build <image>` que genera ejecutables `.exe` | Pendiente |
| **M17**| **Single-EXE Self-Contained** | Empaquetado final de `ubuntu.exe` sin dependencias de archivos auxiliares | Pendiente |
| **M18**| **Windows Guest (WinPE / Win10)**| Soporte para invitados Windows (UEFI, GPT, ACPI, controladores virtuales) | Pendiente |
| **M19**| **BSD Support (FreeBSD)** | Verificación y soporte de arranque de sistemas FreeBSD | Pendiente |
| **M20**| **Multi-Architecture Emulation**| Soporte para invitados ARM64 y RISC-V en backend emulador | Pendiente |
| **M21**| **Capsule GUI Studio & Release**| Interfaz gráfica de construcción y lanzamiento público 1.0 | Pendiente |

---

## Criterios de Validación por Hito

### Hito M0 (objetivo inicial, completado)
- Compilación limpia del workspace con Cargo en Windows x64.
- Ejecutable `capsule.exe` que inicializa una ventana nativa de Windows.
- Detección precisa de:
  - Sistema Operativo host (Windows x64 y versión).
  - Información de CPU (nombre de modelo, núcleos lógicos).
  - Memoria física RAM (total disponible en el sistema).
  - Estado de la API de virtualización WHPX (`WinHvPlatform.dll` presente y API funcional).
- Arquitectura desacoplada en crates para recibir el VMM en M1.

### Hito M8 (completado)
- `capsule-network` implementa `UserModeNat` con ARP (gateway/DNS virtual), DHCP básico (OFFER/ACK), ICMP echo (gateway/DNS virtual) y reenvío UDP saliente mediante sockets del host (flujo UDP pendiente limitado).
- `VirtioNetPci` transitional en PCI legacy I/O (BDF `00:03.0`, BAR0 I/O, MAC en config de dispositivo) con split virtqueues RX/TX en memoria guest y procesamiento por sondeo (sin MMIO, sin drivers host).
- `PciBus` extensible enruta configuración y puertos de E/S a dispositivos PCI y permite sondeo con `GuestMemory`; la VM agrega INTx PCI al PIC como IRQ11 (nivel) junto a PIT IRQ0 e IDE IRQ14.
- Validación: `cargo test --workspace` y pruebas CLI (`test-network`, `test-platform`, `test-cow`, `test-boot-disk`, `test-disk`, `test-storage`, `test-serial`, `test-boot`, `test-guest`).
- Limitaciones: sin inyección de IRQ en CPU (IDT/CLI/STI/IRET), sin MMIO/transporte moderno, sin TCP ni múltiples flujos concurrentes, sin DNS real completo, sin offloads/negociación avanzada.

### Hito M9 (completado)
- `capsule-devices::BochsVbe` implementa registros DISPI por puertos `0x01CE/0x01CF` (ID/modo/XRES/YRES/BPP/enable/bank/offset/video-memory) con latch de modo al habilitar y soporte de LFB.
- LFB modelado en RAM guest en `0x0040_0000` (8 MiB anunciados, sin BAR MMIO dedicado) para compatibilidad con el modelo actual de dispositivos sin MMIO.
- `capsule-vmm` expone `step_once/run_steps` y utilidades de lectura/copia de framebuffer (`vbe_framebuffer_info`, `copy_framebuffer_bgra32`) desde `GuestMemory`.
- `capsule-gui` renderiza el framebuffer en tiempo real en ventana Win32 mediante DIB top-down 32bpp y `StretchDIBits`, ejecutando la VM por pasos en un timer de GUI; modo fallback a diagnósticos si no hay framebuffer activo.
- Validación: `capsule.exe test-framebuffer` (programación VBE, escritura guest en LFB, verificación de píxeles y copia host BGRA32), `cargo test --workspace` y regresión CLI (`test-network`, `test-platform`, `test-cow`, `test-boot-disk`, `test-disk`, `test-storage`, `test-serial`, `test-boot`, `test-guest`).
- Limitaciones: sin MMIO real para LFB/BAR, sin inyección de IRQ/VBlank en CPU (IDT/CLI/STI/IRET), render por sondeo, compatibilidad VBE/parcial orientada a validación inicial.

### Hito M10 (completado)
- `capsule-devices::I8042Ps2Controller` implementa puertos `0x60` (datos) y `0x64` (estado/comando) con cola de salida, comandos bÃ¡sicos de controlador (0x20/0x60/0xA7â€“0xAE/0xAA/0xAB/0xA9/0xD4), y emulaciÃ³n de teclado PS/2 (scancodes Set 1, ACK/reset/identificaciÃ³n bÃ¡sicos) y ratÃ³n PS/2 (modo stream, enable reporting, paquetes de 3 bytes con movimiento/botones y eje Y invertido respecto a pantalla).
- `capsule-vmm` registra el controlador i8042 por defecto, refleja IRQ1 (teclado) e IRQ12 (ratÃ³n) al PIC 8259 en `tick_platform` segÃºn datos pendientes/command byte, refresca estado tras I/O a 0x60/0x64 y expone APIs `inject_ps2_scancodes`, `inject_ps2_mouse_event`, `inject_ps2_mouse_button_packet`.
- `capsule-gui` captura eventos Win32 (`WM_KEYDOWN/UP`, `WM_SYSKEY*`, `WM_MOUSEMOVE`, `WM_L/R/MBUTTON*`, foco) y los traduce a scancodes Set 1/paquetes PS/2 hacia la VM; soporta captura de ratÃ³n en ventana (clic para capturar, Ctrl+Esc para liberar).
- ValidaciÃ³n: `capsule.exe test-input` (lectura guest de scancode por 0x64/0x60, secuencia D4/F4 + ACK + paquete de ratÃ³n, y reflexiÃ³n de IRQ1/IRQ12 a vectores PIC 0x21/0x2C por sondeo), `cargo test --workspace` y regresiones CLI previas.
- Limitaciones: sin inyecciÃ³n de interrupciones en CPU (IDT/CLI/STI/IRET/APIC), sin rueda/5Âº botÃ³n extendidos, mapeo de teclado parcial (Set 1, sin layout completo/IME), captura de ratÃ³n bÃ¡sica y sin dispositivos USB/HID.

### Hito M11 (completado)
- Crate `capsule-whpx` con carga dinamica de `WinHvPlatform.dll` (cuando esta disponible) y wrapper minimo de WHPX: creacion de particion, configuracion basica (1 vCPU), mapeo de `GuestMemory` como GPA (RWX) y ciclo de ejecucion de vCPU (`WHvRunVirtualProcessor`).
- Sincronizacion de registros generales/RIP/RFLAGS y estado inicial minimo de vCPU para ejecucion de guests simples; decodificacion de VM-exits relevantes para el modelo actual, principalmente E/S por puerto x86 (`IN/OUT`) con avance de RIP por longitud de instruccion.
- `capsule-vmm` incorpora seleccion de backend de ejecucion (`VmBackendKind::{Software,Whpx}`), API de acceso a registros unificada (`get/set_regs`, `rip`, `eax`, etc.), `new_whpx()` para forzar WHPX y `new()` con fallback a software si WHPX no esta disponible.
- Las VM-exits WHPX de E/S se despachan a los dispositivos emulados existentes (`VirtualDevice`/PIO) y el tick/sondeo de plataforma (`tick_platform`) se preserva, manteniendo compatibilidad con PIC/PIT/RTC/PS2/PCI/IDE/virtio-net/VBE en el modelo actual. MMIO y entrega arquitectural de IRQ no estan soportados en M11.
- `capsule-gui` intenta backend WHPX por defecto para la sesion GUI/demo (con fallback a software y motivo reportado en UI) y agrega validacion CLI `capsule.exe test-whpx`: en hosts con WHPX ejecuta guest minimo (HLT + OUT a COM1), en hosts sin WHPX reporta SKIP sin fallar.
- Validacion: `cargo test --workspace`, regresion CLI (`test-guest`, `test-boot`, `test-serial`, `test-storage`, `test-disk`, `test-cow`, `test-platform`, `test-network`, `test-framebuffer`, `test-input`, `test-boot-disk`) y `test-whpx` (ejecucion WHPX o SKIP segun host).
- Limitaciones: sin MMIO dispatch, sin inyeccion/entrega arquitectural de interrupciones en CPU (IDT/CLI/STI/IRET/APIC), sin long mode/boot de kernel/firmware real en WHPX (contexto inicial minimo), decodificacion de VM-exit basada en offsets/heuristicas para IO (con fallback por bytes de instruccion) y dependencia de la caracteristica "Windows Hypervisor Platform" habilitada.


