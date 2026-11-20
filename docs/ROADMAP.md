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
| **M9** | **Framebuffer & Window Blit**| Adaptador gráfico VGA/Bochs VBE y renderizado en tiempo real en la ventana Win32 | Pendiente |
| **M10**| **GUI Input & PS/2** | Integración de ratón y teclado desde eventos de ventana hacia controlador PS/2 guest | Pendiente |
| **M11**| **WHPX Hardware Virtualization**| Backend de aceleración por hardware con Windows Hypervisor Platform | Pendiente |
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
