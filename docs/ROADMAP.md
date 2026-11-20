# Hoja de Ruta (Roadmap) de CapsuleOS

Este documento detalla el plan de desarrollo paso a paso para el proyecto CapsuleOS. El desarrollo se realiza de forma estrictamente incremental (hito a hito), donde cada hito debe ser validado y probado antes de avanzar al siguiente.

---

## Resumen de Hitos

| Hito | Nombre | Objetivo Principal | Estado |
|---|---|---|---|
| **M0** | **Host Detection & Window** | Workspace Rust, ventana nativa Win32, detección de CPU, RAM y WHPX | **En progreso** |
| **M1** | **Minimal VM Execution** | VMM elemental, asignación de memoria guest, ejecución de instrucciones simples y detección de HLT | Pendiente |
| **M2** | **CPU Emulator (x86_64)** | Intérprete x86_64 software: registros, decodificación básica, memoria y saltos | Pendiente |
| **M3** | **Direct Linux Boot** | Soporte del protocolo de arranque de Linux (bzImage direct boot) y carga de kernel | Pendiente |
| **M4** | **Linux Console & Shell** | Arranque de Linux con initramfs BusyBox y consola interactiva COM1/UART | Pendiente |
| **M5** | **Virtual Disk (RAW/VHD)** | Controlador de almacenamiento de bloque (IDE / virtio-blk) con imagen de disco | Pendiente |
| **M6** | **Copy-on-Write Persistence** | Persistencia mediante overlay COW sin modificar la imagen base | Pendiente |
| **M7** | **Virtual Platform Devices** | PIT, PIC/APIC, RTC y bus PCI simplificado para soporte de SO completos | Pendiente |
| **M8** | **Networking (User-mode NAT)**| Dispositivo virtio-net con pila SLIRP en modo usuario (acceso a internet sin drivers host) | Pendiente |
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

### Hito M0 (Objetivo Actual)
- Compilación limpia del workspace con Cargo en Windows x64.
- Ejecutable `capsule.exe` que inicializa una ventana nativa de Windows.
- Detección precisa de:
  - Sistema Operativo host (Windows x64 y versión).
  - Información de CPU (nombre de modelo, núcleos lógicos).
  - Memoria física RAM (total disponible en el sistema).
  - Estado de la API de virtualización WHPX (`WinHvPlatform.dll` presente y API funcional).
- Arquitectura desacoplada en crates para recibir el VMM en M1.
