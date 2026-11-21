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
| **M12**| **Firmware BIOS & UEFI** | M12.1 BIOS ROM/stub + boot disco (software); M12.2 ISO/ATAPI/El Torito mínimo; M12.3 OVMF/UEFI condicionado a viabilidad | Completado (scope acotado/condicionado) |
| **M13**| **Ubuntu Minimal / Server** | Slice condicionado: validación de ruta Linux direct boot (kernel/initrd/cmdline) sintética y CLI best-effort para imágenes externas; Ubuntu Server real no declarado soportado | Completado (condicionado/bloqueado) |
| **M14**| **Ubuntu Desktop (GUI)** | Slice condicionado: validación determinista de plumbing GUI (VBE framebuffer + PS/2 input + Linux direct boot); Ubuntu Desktop/X11/Wayland real no soportado | Completado (condicionado/bloqueado) |
| **M15**| **Capsule Bundle Format** | Formato binario (magic/version/compression/manifest/payload), v2 con deflate+CRC32, writer/reader/validación y CLI `test-bundle`; firma/cifrado/streaming pendientes | Completado (scope acotado) |
| **M16**| **Capsule Builder CLI** | CLI `capsule build <input> [out.exe]` / `test-capsule-build`: empaqueta input en bundle y lo appendea a copia del runtime EXE; sin boot real desde payload | Completado (scope acotado) |
| **M17**| **Single-EXE Self-Contained** | EXE autocontenido por append+footer (bundle detectado en el propio EXE); sin compresión/firma/streaming/boot de OS real desde payload | Completado (scope acotado) |
| **M18**| **Windows Guest (WinPE / Win10)**| Slice condicionado/bloqueado: CLI `test-windows-guest` reporta bloqueadores y no declara soporte; sin boot Windows real | Completado (condicionado/bloqueado) |
| **M19**| **BSD Support (FreeBSD)** | Slice condicionado/bloqueado: CLI `test-freebsd` reporta bloqueadores y no declara soporte; sin boot FreeBSD real | Completado (condicionado/bloqueado) |
| **M20**| **Multi-Architecture Emulation**| Slice condicionado: CLI `test-multiarch` declara x86_64 existente y ARM64/RISC-V no implementados | Completado (condicionado/bloqueado) |
| **M21**| **Capsule GUI Studio & Release**| Slice condicionado: CLI `test-studio` checklist honesto; GUI Studio completo y Release 1.0 no declarados | Completado (condicionado/bloqueado) |

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
- Limitaciones: no SeaBIOS/OVMF completo verificado. SeaBIOS 1.16.0 real avanza mÃ¡s en CPU software (ALU/prefixos/segment override/addr32/LEA/string ops/MOVZX/MOVSX/CLC/STC), pero falla reproduciblemente con #UD en RIP=0xE23C; sin soporte declarado. Sigue faltando modo real/segmentaciÃ³n completa, IVT/INT, A20, PCI BIOS/DMA/IRQ delivery en CPU, y ISO/El Torito completo. BIOS externa experimental/best-effort, soportada solo con caso reproducible validado.

#### M12.2 - ISO/ATAPI CD-ROM boot (completado, scope acotado)
- `capsule-devices::IdeController` soporta modo ATAPI mínimo por canal (primary/secondary), sectores 2048 bytes, signature ATAPI, `IDENTIFY PACKET DEVICE` (0xA1), `PACKET` (0xA0), y comandos SCSI básicos: `TEST UNIT READY`, `REQUEST SENSE`, `INQUIRY`, `READ CAPACITY(10)`, `READ(10/12)` (solo LBA 0/count pequeño en path validado), `MODE SENSE(10)` stub.
- IRQ por canal: IDE primario IRQ14, IDE secundario IRQ15, reflejado a PIC por sondeo en `tick_platform`.
- `capsule-vmm` expone `attach_secondary_master_atapi_disk` y `boot_from_cdrom_secondary_master`: parsea Boot Record Volume El Torito (LBA 17, "EL TORITO SPECIFICATION"), lee catalog LBA, valida validation entry y initial/default entry x86 no-emulation (bootable 0x88), carga imagen de boot en 0x7C00 (hasta 2048 bytes para el caso validado) y configura RIP=0x7C00, DL=0xE0. Fallback: carga LBA 0 si no hay El Torito válido.
- BIOS stub integrado actualizado para intentar ATAPI secondary master tras fallo ATA primario (ruta PIO mínima); la ruta principal M12.2 validada es el helper Rust de boot CD-ROM.
- CLI `capsule.exe test-cdrom [iso]`: self-test genera ISO El Torito determinista no-emulation, verifica parseo/carga/ejecución HLT+marker 0xCDB00712; ISO externa es experimental/best-effort.
- Validacion: `cargo test --workspace`, `cargo build --release -p capsule-gui`, regresiones CLI relevantes, `test-firmware`, `test-cdrom`, `test-whpx` (SKIP seguro si host no soporta).
- Limitaciones: ATAPI/El Torito mínimo (no-emulation x86, sin emulación floppy/hdd, sin múltiples entries, sin boot menu). Sin ISO9660 completo, sin INT 13h/extensions CD, sin DMA/UDMA, sin cambio de medio hotplug, sin integrarse con BIOS reales. ISOs de instaladores reales no soportadas.

#### M12.3 - OVMF/UEFI (completado como slice condicionado, NO soporte completo)
- `VmConfig` agrega `firmware_uefi_path` (plumbing de config; sin soporte OVMF completo implícito).
- `capsule-vmm` expone `map_firmware_rom` y `boot_firmware_uefi_image`: mapea imagen UEFI/OVMF en ROM, calcula reset entry determinista (0xFFFF0 cuando aplica) y configura estado mínimo de CPU. No implementa servicios UEFI, GOP, NVRAM, Secure Boot, ni runtime firmware.
- CLI `capsule.exe test-uefi [ovmf.fd]`: self-test determinista con imagen ROM 64KiB (reset vector + código HLT+marker 0x0EF1B007) valida plumbing de mapeo/reset/ejecución. Imagen OVMF externa: best-effort, **no soportada** sin caso reproducible validado.
- Validacion: `cargo test --workspace`, `cargo build --release -p capsule-gui`, regresiones CLI relevantes, `test-firmware`, `test-cdrom`, `test-uefi`, `test-whpx` (SKIP seguro si host no soporta).
- Bloqueadores sin resolver: MMIO, IRQ/APIC/IOAPIC/entrega arquitectural, CPU x86_64 completa (long mode/paginación/excepciones/MSRs/SMM), ACPI/HPET/timers, PCI/firmware devices, GOP/framebuffer UEFI, NVRAM/varstore. **OVMF/UEFI no declarado soportado.**

### Hito M13 (completado como slice condicionado/bloqueado)
- **Objetivo roadmap**: arranque verificado de Ubuntu Server. **Estado real**: Ubuntu Server/Minimal **NO soportado**; no hay caso reproducible validado de boot Ubuntu real en este runtime.
- `VmConfig` añade plumbing de boot Linux directo: `linux_kernel_path`, `linux_initrd_path`, `linux_cmdline` (configuración; no implica soporte de distro real).
- CLI `capsule.exe test-linux-boot [kernel] [initrd] [cmdline]`: ejecuta ruta Linux direct boot existente (`load_linux`/bzImage/setup/zero-page/cmdline/initrd). Sin args usa kernel sintético determinista; con kernel externo es **best-effort** y no declara soporte de OS real.
- CLI `capsule.exe test-ubuntu-server [kernel] [initrd] [cmdline]`: slice condicionado M13. Sin imagen externa valida solo ruta sintética de boot Linux; con imagen externa reporta best-effort y bloqueadores. **Nunca declara Ubuntu Server soportado** sin caso reproducible validado.
- Validación: `cargo test --workspace`, `cargo build --release -p capsule-gui`, `test-linux-boot`, `test-ubuntu-server`, regresiones CLI (`test-guest`, `test-boot`, `test-serial`, `test-storage`, `test-disk`, `test-cow`, `test-platform`, `test-network`, `test-framebuffer`, `test-input`, `test-boot-disk`, `test-firmware`, `test-cdrom`, `test-uefi`, `test-whpx` SKIP seguro).
- **Bloqueadores para Ubuntu real**: MMIO, entrega arquitectural IRQ/APIC/IOAPIC/IDT/CLI/STI/IRET, CPU x86_64 completa (long mode/paginación/excepciones/MSRs), timers HPET/APIC/ACPI, dispositivos PCI/virtio completos, firmware/UEFI real, almacenamiento/red/controladores compatibles con Linux moderno.

### Hito M14 (completado como slice condicionado/bloqueado)
- **Objetivo roadmap**: arranque verificado de Ubuntu Desktop con X11/Wayland en ventana. **Estado real**: **NO soportado**; no hay caso reproducible validado de Ubuntu Desktop real.
- CLI `capsule.exe test-linux-gui [kernel] [initrd] [cmdline]`: slice condicionado que valida plumbing GUI determinista reusando `test-framebuffer` (VBE/LFB + blit host) y `test-input` (PS/2 teclado/ratón/IRQ por sondeo), más ruta Linux direct boot sintética/best-effort.
- CLI `capsule.exe test-ubuntu-desktop [kernel] [initrd] [cmdline]`: mismo slice con etiquetado Ubuntu Desktop; sin args ejecuta validación sintética, con args externos es best-effort y **nunca declara Ubuntu Desktop soportado**.
- La GUI Win32 existente ya renderiza LFB VBE y traduce eventos teclado/ratón a PS/2 (M9/M10); esto es plumbing, no soporte de desktop Linux real.
- Validación: `cargo test --workspace`, `cargo build --release -p capsule-gui`, `test-linux-gui`, `test-ubuntu-desktop`, regresiones CLI clave y `test-whpx` SKIP seguro.
- **Bloqueadores para Ubuntu Desktop real**: MMIO, IRQ/APIC/IOAPIC/IDT delivery, CPU x86_64 completa, timers ACPI/HPET/APIC, GPU/virtio-gpu/drm/kms, UEFI/firmware, USB/HID completo, almacenamiento/red/driver stack Linux completo.

### Hito M15 (completado, scope acotado)
- Crate `capsule-bundle` con formato binario: magic `CAPSULE\0`, v2, compresión (`None`/`Deflate`), longitudes, CRC32 manifest/payload, manifest UTF-8 key=value, payload. v1 legacy legible.
- `CapsuleManifest` incluye `name`, `guest_os`, `backend`, `memory_mib`, `vcpus`, `payload_kind`, `kernel_cmdline`, `initrd_bytes`, `disk_sector_size`. API: `write_to_file`/`read_from_file`/`from_bytes`, validación de magic/version/compression/truncation/CRC.
- CLI `capsule.exe test-bundle [path]`: roundtrip determinista, deflate+CRC32, prueba de corrupción.
- Validación: `cargo test -p capsule-bundle`, `cargo test --workspace`, `cargo build --release -p capsule-gui`, `test-bundle`.
- Limitaciones: sin firma criptográfica/cifrado, sin streaming por bloques, sin manifiesto completo de boot multi-artifact.
### Hito M16/M17 (completado, scope acotado)
- `capsule-bundle` añade append/detección en EXE: `append_to_exe`, `read_from_appended_exe`, footer `CAPSLEXE + bundle_len + version + reserved`.
- CLI `capsule.exe build <input> [out.exe]` y `capsule.exe test-capsule-build [input] [out.exe]`: copia el runtime EXE actual, appendea bundle con payload del input, y valida que el bundle appendado se puede leer desde el EXE generado.
- Esto produce un artefacto single-file por append (M17 acotado), pero **no** boot real de OS desde payload ni empaquetado seguro/productivo completo.
- Validación: `cargo test -p capsule-bundle`, `cargo test --workspace`, `cargo build --release -p capsule-gui`, `test-capsule-build`, `test-bundle`, `test-whpx` SKIP seguro.
- Limitaciones: sin compresión, firma/validación criptográfica, streaming, alineación/firmado PE avanzado, manifest completo de boot, selección automática de payload por VM, ni soporte real de Ubuntu/Windows guest desde bundle.

### Hito M18 (completado, condicionado/bloqueado)
- **Objetivo roadmap**: soporte Windows Guest (WinPE/Win10). **Estado real**: **NO soportado**.
- CLI `capsule.exe test-windows-guest [image]`: inspección metadata opcional, reporte determinista de bloqueadores; no intenta boot. No declara soporte.
- Bloqueadores: UEFI/OVMF completo, ACPI/GPT, MMIO, IRQ/APIC/IOAPIC, CPU x86_64 completa, timers/HPET, storage/network/GPU drivers, Secure Boot/NVRAM.

### Hito M19 (completado, condicionado/bloqueado)
- **Objetivo roadmap**: soporte FreeBSD. **Estado real**: **NO soportado**.
- CLI `capsule.exe test-freebsd [image]`: inspección metadata opcional, reporte determinista de bloqueadores; no intenta boot. No declara soporte.
- Bloqueadores: bootloader/UEFI/BIOS compatible, MMIO, IRQ/APIC/IOAPIC, CPU x86_64 completa, timers/ACPI/HPET, virtio/AHCI/red/drivers.

### Hito M20 (completado, condicionado/bloqueado)
- **Objetivo roadmap**: multi-arch ARM64/RISC-V. **Estado real**: solo x86_64 existente; ARM64/RISC-V **NO implementados**.
- CLI `capsule.exe test-multiarch`: reporta alcance actual y gaps por arquitectura. No declara emulación multi-arch.
- Bloqueadores: decodificadores/emuladores CPU, MMU/modelos memoria, interrupciones, firmware/boot, dispositivos y ABIs por arquitectura.

### Hito M21 (completado, condicionado/bloqueado)
- **Objetivo roadmap**: GUI Studio & Release 1.0. **Estado real**: GUI base y CLIs existen; **Studio completo y Release 1.0 NO declarados**.
- CLI `capsule.exe test-studio`: checklist honesto de capacidades/ausencias de release.
- Bloqueadores: soporte OS reales, MMIO/IRQ/CPU completa, firmware/devices robustos, UX studio, packaging seguro, QA end-to-end, signing/distribution.
