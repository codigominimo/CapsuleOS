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
| **M10**| **GUI Input & PS/2** | Controlador PS/2 i8042 (0x60/0x64) con teclado/ratón, IRQ1/IRQ12 reflejadas al PIC por sondeo, inyección de eventos Win32 desde GUI y validación `test-input` | **Completado** |
| **M11**| **WHPX Hardware Virtualization**| Backend de aceleración por hardware con Windows Hypervisor Platform (WHPX), mapeo GPA y VM-exits de E/S hacia dispositivos emulados, con fallback a software | **Completado** |
| **M12**| **Firmware BIOS & UEFI** | M12.1 BIOS ROM/stub + boot disco (software); M12.2 ISO/ATAPI/El Torito mínimo; M12.3 OVMF/UEFI condicionado a viabilidad | En progreso (M12.1-M12.2 completados, scope acotado) |
| **M13**| **Ubuntu Minimal / Server** | Arranque verificado de distribución Ubuntu Server mediante WHPX | Pendiente |
| **M14**| **Ubuntu Desktop (GUI)** | Arranque verificado de entorno gráfico X11/Wayland de Ubuntu en ventana | Pendiente |
| **M15**| **Capsule Bundle Format** | Especificación del contenedor monolítico: Header, Manifest, Payload comprimido | Pendiente |
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
- `capsule-devices::I8042Ps2Controller` implementa puertos `0x60` (datos) y `0x64` (estado/comando) con cola de salida, comandos básicos de controlador (0x20/0x60/0xA7-0xAE/0xAA/0xAB/0xA9/0xD4), y emulación de teclado PS/2 (scancodes Set 1, ACK/reset/identificación básicos) y ratón PS/2 (modo stream, enable reporting, paquetes de 3 bytes con movimiento/botones y eje Y invertido respecto a pantalla).
- `capsule-vmm` registra el controlador i8042 por defecto, refleja IRQ1 (teclado) e IRQ12 (ratón) al PIC 8259 en `tick_platform` según datos pendientes/command byte, refresca estado tras I/O a 0x60/0x64 y expone APIs `inject_ps2_scancodes`, `inject_ps2_mouse_event`, `inject_ps2_mouse_button_packet`.
- `capsule-gui` captura eventos Win32 (`WM_KEYDOWN/UP`, `WM_SYSKEY*`, `WM_MOUSEMOVE`, `WM_L/R/MBUTTON*`, foco) y los traduce a scancodes Set 1/paquetes PS/2 hacia la VM; soporta captura de ratón en ventana (clic para capturar, Ctrl+Esc para liberar).
- Validación: `capsule.exe test-input` (lectura guest de scancode por 0x64/0x60, secuencia D4/F4 + ACK + paquete de ratón, y reflexión de IRQ1/IRQ12 a vectores PIC 0x21/0x2C por sondeo), `cargo test --workspace` y regresiones CLI previas.
- Limitaciones: sin inyección de interrupciones en CPU (IDT/CLI/STI/IRET/APIC), sin rueda/5º botón extendidos, mapeo de teclado parcial (Set 1, sin layout completo/IME), captura de ratón básica y sin dispositivos USB/HID.

### Hito M11 (completado)
- Crate `capsule-whpx` con carga dinámica de `WinHvPlatform.dll` (cuando está disponible) y wrapper mínimo de WHPX: creación de partición, configuración básica (1 vCPU), mapeo de `GuestMemory` como GPA (RWX) y ciclo de ejecución de vCPU (`WHvRunVirtualProcessor`).
- Sincronización de registros generales/RIP/RFLAGS y estado inicial mínimo de vCPU para ejecución de guests simples; decodificación de VM-exits relevantes para el modelo actual, principalmente E/S por puerto x86 (`IN/OUT`) con avance de RIP por longitud de instrucción.
- `capsule-vmm` incorpora selección de backend de ejecución (`VmBackendKind::{Software,Whpx}`), API de acceso a registros unificada (`get/set_regs`, `rip`, `eax`, etc.), `new_whpx()` para forzar WHPX y `new()` con fallback a software si WHPX no está disponible.
- Las VM-exits WHPX de E/S se despachan a los dispositivos emulados existentes (`VirtualDevice`/PIO) y el tick/sondeo de plataforma (`tick_platform`) se preserva, manteniendo compatibilidad con PIC/PIT/RTC/PS2/PCI/IDE/virtio-net/VBE en el modelo actual. MMIO y entrega arquitectural de IRQ no están soportados en M11.
- `capsule-gui` intenta backend WHPX por defecto para la sesión GUI/demo (con fallback a software y motivo reportado en UI) y agrega validación CLI `capsule.exe test-whpx`: en hosts con WHPX ejecuta guest mínimo (HLT + OUT a COM1), en hosts sin WHPX o con inicialización no utilizable reporta SKIP sin fallar.
- Validación: `cargo test --workspace`, regresión CLI (`test-guest`, `test-boot`, `test-serial`, `test-storage`, `test-disk`, `test-cow`, `test-platform`, `test-network`, `test-framebuffer`, `test-input`, `test-boot-disk`) y `test-whpx` (ejecución WHPX o SKIP según host).
- Limitaciones: sin MMIO dispatch, sin inyección/entrega arquitectural de interrupciones en CPU (IDT/CLI/STI/IRET/APIC), sin long mode/boot de kernel/firmware real en WHPX (contexto inicial mínimo), decodificación de VM-exit basada en offsets/heurísticas para IO (con fallback por bytes de instrucción) y dependencia de la característica "Windows Hypervisor Platform" habilitada.

### Hito M12 (en progreso)
#### M12.1 - Firmware BIOS ROM / boot desde disco en software (completado, scope acotado)
- `capsule-memory` soporta mapeo ROM (`map_rom`) y regiones ROM de solo lectura; escrituras a ROM se ignoran.
- `capsule-vmm` expone `map_bios_rom` y `boot_firmware_from_primary_master_disk` (disco IDE primario, BIOS externa opcional o stub integrado, reset minimo RIP/RSP/RFLAGS/DL=0x80).
- BIOS stub integrado determinista 64 KiB (M12.1, backend software): banner COM1, lee LBA0 via ATA PIO a 0x7C00 y salta al boot sector. No implementa INT BIOS reales; usa modelo plano/ISA soportado por el interprete.
- `VmConfig.firmware_bios_path`; CLI `capsule.exe test-firmware [bios_path]` valida firma 0x55AA en 0x7C00 y ejecucion del boot sector.
- `capsule-cpu` amplia ISA minima para firmware/stub (ALU AL,imm8; 0x80 r/m8,imm8; TEST r/m8,r8; CLI/STI/CLD/STD NOP compat; JMP/CALL indirectos FF/4,/2).
- Validacion: `cargo test --workspace`, `cargo build --release -p capsule-gui`, regresiones CLI relevantes y `test-firmware`.
- Limitaciones: no SeaBIOS/OVMF completo verificado; sin modo real/segmentacion, IVT/INT, A20, PCI BIOS/DMA/IRQ delivery en CPU, ni ISO/El Torito completo. BIOS externa experimental/best-effort, soportada solo con caso reproducible validado.

#### M12.2 - ISO/ATAPI CD-ROM boot (completado, scope acotado)
- `capsule-devices::IdeController` soporta modo ATAPI mínimo por canal (primary/secondary), sectores 2048 bytes, signature ATAPI, `IDENTIFY PACKET DEVICE` (0xA1), `PACKET` (0xA0), y comandos SCSI básicos: `TEST UNIT READY`, `REQUEST SENSE`, `INQUIRY`, `READ CAPACITY(10)`, `READ(10/12)` (solo LBA 0/count pequeño en path validado), `MODE SENSE(10)` stub.
- IRQ por canal: IDE primario IRQ14, IDE secundario IRQ15, reflejado a PIC por sondeo en `tick_platform`.
- `capsule-vmm` expone `attach_secondary_master_atapi_disk` y `boot_from_cdrom_secondary_master`: parsea Boot Record Volume El Torito (LBA 17, "EL TORITO SPECIFICATION"), lee catalog LBA, valida validation entry y initial/default entry x86 no-emulation (bootable 0x88), carga imagen de boot en 0x7C00 (hasta 2048 bytes para el caso validado) y configura RIP=0x7C00, DL=0xE0. Fallback: carga LBA 0 si no hay El Torito válido.
- BIOS stub integrado actualizado para intentar ATAPI secondary master tras fallo ATA primario (ruta PIO mínima); la ruta principal M12.2 validada es el helper Rust de boot CD-ROM.
- CLI `capsule.exe test-cdrom [iso]`: self-test genera ISO El Torito determinista no-emulation, verifica parseo/carga/ejecución HLT+marker 0xCDB00712; ISO externa es experimental/best-effort.
- Validacion: `cargo test --workspace`, `cargo build --release -p capsule-gui`, regresiones CLI relevantes, `test-firmware`, `test-cdrom`, `test-whpx` (SKIP seguro si host no soporta).
- Limitaciones: ATAPI/El Torito mínimo (no-emulation x86, sin emulación floppy/hdd, sin múltiples entries, sin boot menu). Sin ISO9660 completo, sin INT 13h/extensions CD, sin DMA/UDMA, sin cambio de medio hotplug, sin integrarse con BIOS reales. ISOs de instaladores reales no soportadas.

#### M12.3 - OVMF/UEFI (pendiente, condicionado)
- Objetivo: carga UEFI/OVMF y ruta de boot reproducible. No se declarara soportado sin caso reproducible validado.
- Bloqueadores: MMIO, IRQ/APIC/IOAPIC, CPU x86_64 mas completa (modos/paginacion/excepciones/MSRs), timers ACPI/HPET, dispositivos firmware/PCI más completos.
