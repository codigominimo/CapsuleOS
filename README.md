# CapsuleOS

> **Sistemas operativos completos dentro de una cápsula ejecutable portable `.exe` para Windows.**

CapsuleOS es un entorno de ejecución y empaquetado para Windows escrito en Rust que permite ejecutar sistemas operativos completos (Ubuntu, Debian, Alpine, FreeBSD, Windows, etc.) como si fueran aplicaciones portables independientes de Windows.

```text
Ubuntu.exe   → arranca Ubuntu
Debian.exe   → arranca Debian
Alpine.exe   → arranca Alpine
FreeBSD.exe  → arranca FreeBSD
Windows.exe  → arranca Windows
```

El usuario final simplemente hace doble clic sobre el `.exe` y obtiene una ventana con el sistema operativo real ejecutándose dentro de una máquina virtual. **No es necesario instalar previamente VirtualBox, VMware ni QEMU.** CapsuleOS proporciona su propio runtime.

---

## 🏛️ Diferenciación Clave

| Entorno Tradicional (VirtualBox / VMware) | CapsuleOS |
|---|---|
| Hipervisor externo pesado instalado en el SO | Runtime autónomo integrado en el propio `.exe` |
| Asistentes de configuración manual de VMs | Cero configuración: doble clic y arranque directo |
| Archivos dispersos (`.vdi`, `.vmx`, `.vbox`, etc.) | Cápsula autocontenida (`Ubuntu.exe`) |
| Dependencia de drivers de kernel de terceros | Uso de APIs nativas de Windows (**WHPX**) o emulador software |

---

## ⚡ Backends de Ejecución

CapsuleOS implementa una arquitectura híbrida de doble motor:

```text
                  CapsuleOS
                      │
          ┌───────────┴───────────┐
          │                       │
     Hardware VM              Emulador
          │                       │
     WHPX (VT-x/SVM)        Software x86_64
          │                       │
          └───────────┬───────────┘
                      │
                   Guest OS
```

1. **Aceleración por Hardware (WHPX)**: Utiliza la API nativa *Windows Hypervisor Platform* (`WinHvPlatform.dll`) cuando está habilitada en Windows, ofreciendo rendimiento casi nativo.
2. **Emulación por Software**: Intérprete/emulador de CPU x86-64 puro para entornos donde la virtualización por hardware no esté disponible o esté deshabilitada.

---

## 📦 Estructura del Cargo Workspace

El proyecto está diseñado de forma estrictamente modular y desacoplada:

```text
CapsuleOS/
├── Cargo.toml                      # Definición del Workspace
├── docs/                           # Especificaciones y análisis técnico
│   ├── ARCHITECTURE.md             # Arquitectura detallada de subsistemas
│   ├── ROADMAP.md                  # Hoja de ruta completa de hitos (M0 a M21)
│   ├── COMPATIBILITY.md            # Matriz de compatibilidad hosts y guests
│   └── TECHNICAL_LIMITATIONS.md    # Restricciones PE, WHPX y persistencia
├── crates/
│   ├── capsule-core/               # Primitivas, direcciones GPA/HVA e introspección host
│   ├── capsule-memory/             # Gestión de memoria física guest con VirtualAlloc
│   ├── capsule-cpu/                # Registros arquitecturales x86-64 y vCPU elemental
│   ├── capsule-devices/            # Dispositivos de plataforma (UART, PIC/PIT/RTC, PCI, IDE, virtio-net)
│   ├── capsule-network/            # Pila NAT en modo usuario para red guest
│   ├── capsule-storage/            # Discos RAW/VHD y overlay COW
│   ├── capsule-vmm/                # Orquestador de la VM, ciclo de vida y dispositivos
│   ├── capsule-whpx/               # Backend de virtualización por hardware WHPX (WinHvPlatform)
│   └── capsule-gui/                # Aplicación ejecutable (capsule.exe) y ventana Win32
```

---

## Estado Actual de Desarrollo

El desarrollo se realiza de forma estrictamente incremental por hitos comprobados y probados:

* [x] **Hito M0 — Host Diagnostics & Ventana Win32**:
  - Detección precisa de Windows x64 (Edición y Build).
  - Detección de CPU (Nombre de modelo, fabricante, núcleos e instrucciones VT-x/SVM/Hypervisor).
  - Detección de memoria RAM física total, disponible y porcentaje de carga.
  - Detección dinámica en caliente de WHPX (`WinHvPlatform.dll`).
  - Ventana nativa Win32 optimizada (ClearType, interfaz oscura, cero frameworks pesados).
* [x] **Hito M1 — VM Mínima & Ejecución Guest**:
  - Asignación segura de memoria física guest (`GuestMemory`).
  - Carga de código máquina x86-64 en GPA.
  - Ejecución de vCPU paso a paso y detección de salidas VM (`HLT`, `I/O Port Out`).
  - Comando CLI interactivo: `capsule.exe test-guest`.
* [x] **Hito M2 — Expansión de CPU Emulada (x86_64)**:
  - Decodificación completa de instrucciones: aritmética, lógica, ModR/M, SIB.
  - Prefijos REX para operaciones en modo de 64 bits completo y registros `R8`-`R15`.
  - Banderas `RFLAGS` (`ZF`, `CF`, `SF`, `OF`, `PF`), saltos condicionales `Jcc` y pila (`CALL`/`RET`).
* [x] **Hito M3 - Arranque Directo del Kernel Linux (bzImage direct boot)**:
  - Parser del encabezado `SetupHeader` con firma `HdrS`.
  - Construcción de la *Zero-Page* (`boot_params`) y mapa de memoria física E820.
  - Creación de tablas de páginas de 4 niveles (PML4) con *identity mapping* de 1 GB en páginas de 2MB.
  - Carga de línea de comandos del kernel e initramfs y ejecución guest vía protocolo de arranque directo.
  - Comando CLI de validación: `capsule.exe test-boot`.
* [x] **Hito M4 - Linux Console & Shell (base UART / earlyprintk)**:
  - Emulación UART 16550 (COM1, 0x3F8) integrada como dispositivo virtual de la VM.
  - Soporte de E/S por puerto para salida de consola guest (`console=ttyS0` / earlyprintk).
  - Comando CLI de validación: `capsule.exe test-serial`.
* [x] **Hito M5 - Virtual Disk (RAW/VHD)**:
  - Abstracción `BlockDevice` y backends `MemoryDisk`, `RawDiskImage`, `VhdFixedDisk` (fixed).
  - Controlador ATA/IDE PIO primario (canal maestro, LBA28): `IDENTIFY`, `READ/WRITE SECTORS` (y `READ/WRITE MULTIPLE`), `SET MULTIPLE MODE` (compat.), `FLUSH CACHE`.
  - Modelo de IRQ simplificado (condición pendiente, `nIEN`, status vs alt-status) y software reset (`SRST`), con manejo de unidad ausente/slave y errores básicos (`IDNF`/`ABRT`).
  - Integración VM: `attach_primary_master_disk`, E/S PIO 8/16/32-bit y arranque mínimo desde disco (`boot_from_primary_master_disk`: carga LBA0 en `0x7C00`, `RIP=0x7C00`).
  - Validación: transferencias PIO (incl. multi-sector y `sector_count=0` => 256 sectores), guest READ/WRITE/FLUSH con persistencia en RAW/VHD fixed, y boot sector con carga multi-sector vía IDE.
  - Comandos CLI: `capsule.exe test-storage`, `capsule.exe test-disk`, `capsule.exe test-boot-disk [path]` (self-test o arranque experimental de imagen RAW/VHD fixed 512-byte).
  - Alcance diferido: inyección de interrupciones en la CPU software (IDT/CLI/STI/IRET) para atender IRQs del PIC/PIT/IDE; mayor compatibilidad ATA/drivers y arranque real desde disco/firmware (M12); y formatos dinámicos o `virtio-blk`.
* [x] **Hito M6 - Copy-on-Write Persistence**:
  - Overlay COW (`CowOverlayDisk`) sobre cualquier `BlockDevice` base (p. ej. RAW, VHD fixed), con bitmap por sector y persistencia en archivo de overlay.
  - Lectura transparente: sectores no modificados se leen de la base; sectores escritos se resuelven desde el overlay. Las escrituras nunca modifican la imagen base.
  - Validación de geometría base/overlay, reapertura con persistencia de cambios y operación a través del stack ATA/IDE+VM (guest READ/WRITE/FLUSH) sin alterar la base.
  - Comando CLI de validación: `capsule.exe test-cow`.
* [x] **Hito M7 - Virtual Platform Devices (PIC/PIT/RTC/PCI)**:
  - PIC 8259 dual (maestro/esclavo) con ICW/OCW, IMR/ISR/IRR, cascada y EOI; IRQ0 (PIT) e IRQ14 (IDE primario, nivel según `irq_pending && !nIEN`) reflejadas en el PIC.
  - PIT 8254 (canales 0-2, modos básicos) con avance por tick de vCPU y pulsos IRQ0 (canal 0).
  - RTC/CMOS MC146818 mínimo (puertos `0x70/0x71`): hora/fecha básica y RAM CMOS preservada.
  - Bus PCI extensible (mecanismo #1, `0xCF8/0xCFC`) con enrutamiento de configuración/E/S y sondeo de dispositivos con acceso a memoria guest.
  - Integración en VM: dispositivos de plataforma registrados por defecto y refresco de timers/IRQs en el bucle de ejecución.
  - Comando CLI de validación: `capsule.exe test-platform`.
  - Nota: la inyección de interrupciones en la CPU software (IDT/CLI/STI/IRET) no está implementada aún; los guests deben usar sondeo (polling).
* [x] **Hito M8 - Networking (User-mode NAT / virtio-net)**:
  - Crate `capsule-network` con `UserModeNat`: ARP para gateway/DNS virtual, DHCP básico (OFFER/ACK), ICMP echo (gateway/DNS virtual) y reenvío UDP saliente mediante sockets del host (flujo UDP pendiente limitado).
  - Dispositivo `VirtioNetPci` transitional PCI legacy (I/O BAR0, split virtqueues RX/TX en memoria guest) en BDF `00:03.0`, operado por sondeo desde la VM (sin MMIO ni drivers host).
  - IRQ INTx PCI agregada señalada en nivel al PIC como IRQ11; procesamiento de virtqueues/NAT realizado en el bucle de ejecución.
  - Comando CLI de validación: `capsule.exe test-network`.
  - Alcance diferido: TCP, múltiples flujos/conexiones concurrentes, DNS real completo, features/offloads avanzados, inyección de IRQ en CPU (IDT/CLI/STI/IRET) y transporte moderno/MMIO.
* [x] **Hito M9 - Framebuffer & Window Blit (VGA/Bochs VBE)**:
  - Dispositivo `BochsVbe` (puertos DISPI `0x01CE/0x01CF`) con programación de modo/resolución/bpp y habilitación de LFB.
  - Framebuffer lineal mapeado en RAM guest en `0x0040_0000` (modelo sin MMIO, compatible con el backend software actual) y acceso desde `GuestMemory`.
  - Integración VMM/GUI: `step_once/run_steps` y copia de framebuffer a buffer BGRA32 para blit Win32 (`StretchDIBits`), renderizado en tiempo real en la ventana de `capsule.exe`.
  - Comando CLI de validación: `capsule.exe test-framebuffer`.
  - Limitaciones: LFB en RAM guest (no BAR MMIO dedicado), sin inyección de IRQ en CPU (IDT/CLI/STI/IRET), backend de render por sondeo/timer.
* [x] **Hito M10 - GUI Input & PS/2 (i8042)**:
  - Controlador PS/2 i8042 (`0x60/0x64`) en `capsule-devices` con teclado (scancodes Set 1) y ratón PS/2 (modo stream, paquetes de 3 bytes), integrado en la VM y expuesto mediante APIs de inyección desde `capsule-vmm`.
  - IRQ1 (teclado) e IRQ12 (ratón) reflejadas al PIC 8259 por sondeo en `tick_platform` según datos pendientes/command byte (sin inyección de interrupciones en la CPU software).
  - `capsule-gui` captura eventos Win32 (`WM_KEYDOWN/UP`, `WM_SYSKEY*`, `WM_MOUSEMOVE`, `WM_*BUTTON*`, foco) y los traduce a scancodes/paquetes PS/2; captura/liberación de ratón en ventana (clic para capturar, Ctrl+Esc para liberar).
  - Comando CLI de validación: `capsule.exe test-input`.
  - Limitaciones: sin inyección de IRQ en CPU (IDT/CLI/STI/IRET/APIC), mapeo de teclado parcial (Set 1, sin layout completo/IME), sin rueda/botones extendidos ni USB/HID, captura de ratón básica.
* [x] **Hito M11 - WHPX Hardware Virtualization**:
  - Crate `capsule-whpx` con carga dinámica de `WinHvPlatform.dll` y wrapper mínimo de WHPX: partición, vCPU, mapeo de `GuestMemory` como GPA, ejecución `WHvRunVirtualProcessor` y sincronización básica de registros.
  - `capsule-vmm` soporta selección de backend (`VmBackendKind::{Software,Whpx}`) con API unificada de registros; `VirtualMachine::new()` intenta WHPX cuando se solicita y hace fallback a software si no está disponible, y `new_whpx()` fuerza WHPX para validación explícita.
  - VM-exits de E/S por puerto (IN/OUT) se traducen al despacho de dispositivos PIO/tick de plataforma existente, preservando el modelo actual de emulación de dispositivos.
  - `capsule-gui` intenta WHPX por defecto en la GUI/demo (con fallback y reporte de estado) y agrega `capsule.exe test-whpx` (guest mínimo HLT + IO a COM1 en WHPX, o SKIP si el host no lo soporta).
  - Limitaciones: sin MMIO, sin inyección/entrega arquitectural de IRQ (IDT/CLI/STI/IRET/APIC), contexto inicial mínimo (sin long mode/firmware/kernel real), y fidelidad/temporización dependiente del modelo de tick/sondeo actual.
* [x] **Hito M12 - Firmware BIOS & UEFI** (completado, scope acotado/condicionado):
  - **M12.1**: ROM mapeada en `GuestMemory` (writes ignoradas); `capsule-vmm` mapea BIOS y hace boot firmware desde disco IDE primario con reset minimo. BIOS stub integrado 64 KiB (software) imprime banner COM1, lee LBA0 por ATA PIO a 0x7C00 y salta al boot sector. `VmConfig.firmware_bios_path` permite BIOS externa experimental. `capsule.exe test-firmware [bios.bin]`.
  - **M12.2**: ATAPI PIO mínimo en IDE secundario maestro (0x170/IRQ15), sectores 2048, boot CD-ROM ISO. `boot_from_cdrom_secondary_master` parsea El Torito (Boot Record Volume, catalog, initial/default x86 no-emulation), carga imagen boot en 0x7C00, DL=0xE0; fallback LBA0. `capsule.exe test-cdrom [iso]`.
  - **M12.3 (condicionado, NO soporte OVMF completo)**: `VmConfig.firmware_uefi_path`, `map_firmware_rom`/`boot_firmware_uefi_image` para mapeo ROM + reset determinista. `capsule.exe test-uefi [ovmf.fd]` valida plumbing con stub determinista; OVMF externo best-effort/no soportado sin caso reproducible validado.
  - Limitaciones: no SeaBIOS/OVMF completo; sin modo real/segmentacion, IVT/INT BIOS, INT 13h CD, A20, PCI BIOS, DMA/UDMA, IRQ delivery arquitectural. ATAPI/El Torito mínimo. UEFI bloqueado por MMIO, IRQ/APIC/IOAPIC, CPU x86_64 completa, timers/ACPI/firmware devices.

* [~] **Hito M13 - Ubuntu Minimal / Server** (completado como slice condicionado/bloqueado):
  - `VmConfig` agrega plumbing de Linux direct boot: `linux_kernel_path`, `linux_initrd_path`, `linux_cmdline`.
  - `capsule.exe test-linux-boot [kernel] [initrd] [cmdline]`: valida ruta Linux direct boot (`load_linux`, bzImage/setup/zero-page/cmdline/initrd). Sin args usa kernel sintético determinista; con kernel externo es best-effort.
  - `capsule.exe test-ubuntu-server [kernel] [initrd] [cmdline]`: slice M13 condicionado. **Ubuntu Server/Minimal NO declarado soportado** sin caso reproducible validado.
  - Bloqueadores: MMIO, IRQ/APIC/IOAPIC/IDT delivery completo, CPU x86_64 completa (long mode/paginación/excepciones/MSRs), timers ACPI/HPET/APIC, PCI/virtio/drivers completos, firmware/UEFI real.

* [~] **Hito M14 - Ubuntu Desktop (GUI)** (completado como slice condicionado/bloqueado):
  - `capsule.exe test-linux-gui [kernel] [initrd] [cmdline]`: valida plumbing GUI determinista (VBE framebuffer + blit host + PS/2 input/IRQ por sondeo) y ruta Linux direct boot sintética/best-effort.
  - `capsule.exe test-ubuntu-desktop [kernel] [initrd] [cmdline]`: slice M14 condicionado. **Ubuntu Desktop/X11/Wayland NO declarado soportado** sin caso reproducible validado.
  - Bloqueadores: MMIO, IRQ/APIC/IOAPIC/IDT delivery, CPU x86_64 completa, timers ACPI/HPET/APIC, GPU/virtio-gpu/drm/kms, UEFI/firmware, USB/HID y stack Linux completo.

---

## Compilación y Uso

### Requisitos
- Windows 10 o Windows 11 (x64).
- Rust 1.80+ (`rustup default stable-x86_64-pc-windows-msvc`).

### Compilar el proyecto
```powershell
# Compilación en modo release
cargo build --release
```
El ejecutable resultante (`target\release\capsule.exe`) tiene un tamaño ligero de aproximadamente **200 KB**.

### Ejecutar la prueba de VM Mínima (Hito M1)
```powershell
.\target\release\capsule.exe test-guest
```
Salida esperada:
```text
VM started
CPU started
Guest executed
Guest halted

--------------------------------------------------
        Verificación de Ejecución Guest M1        
--------------------------------------------------
  Resultado en EAX  : 0x00000053 (83 decimal)
  RIP final         : 0x000000000000000B
  Estado de la VM   : Halted
--------------------------------------------------
```

### Ejecutar la aplicación gráfica (Hito M0)
```powershell
.\target\release\capsule.exe
```
Abre la ventana nativa de diagnóstico con la información del host y estado del runtime.

### Verificar almacenamiento de bloques (Hito M5)
```powershell
.\target\release\capsule.exe test-storage
```

### Verificar disco ATA/IDE adjunto a la VM (Hito M5)
```powershell
.\target\release\capsule.exe test-disk
```

### Verificar arranque desde disco (sector de arranque + IDE PIO, Hito M5)
```powershell
# Self-test sintético (LBA0 -> 0x7C00, carga multi-sector LBA1-2 via ATA PIO)
.\target\release\capsule.exe test-boot-disk

# Arranque experimental de una imagen externa RAW o VHD fixed (sector 512 bytes)
.\target\release\capsule.exe test-boot-disk .\ruta\disco.img
```

### Verificar persistencia Copy-on-Write (overlay COW, Hito M6)
```powershell
# Guest ATA PIO READ/WRITE/FLUSH sobre overlay COW (bases RAW y VHD fixed)
# Valida que la imagen base permanezca intacta y los cambios persistan en el overlay
.\target\release\capsule.exe test-cow
```

### Verificar plataforma virtual (PIC/PIT/RTC/PCI, Hito M7)
```powershell
# Valida PIC 8259, PIT 8254, RTC/CMOS y bus PCI (config 0xCF8/0xCFC), e integración básica en VM
.\target\release\capsule.exe test-platform
```

### Verificar red en modo usuario (NAT + virtio-net PCI, Hito M8)
```powershell
# Valida ARP/ICMP en NAT de usuario y enumeración/configuración básica de virtio-net (00:03.0, BAR0 I/O, MAC)
.\target\release\capsule.exe test-network
```

### Verificar framebuffer VGA/Bochs VBE y blit Win32 (Hito M9)
```powershell
# Programa VBE por puertos I/O, escribe en LFB guest (0x0040_0000) y valida copia BGRA32 para render
.\target\release\capsule.exe test-framebuffer
```

### Verificar entrada PS/2 i8042 (teclado/ratón, Hito M10)
```powershell
# Valida scancodes por 0x64/0x60, secuencia D4/F4+ACK+paquete de ratón e IRQ1/IRQ12 reflejadas al PIC (vectores 0x21/0x2C) por sondeo
.\target\release\capsule.exe test-input
```

### Verificar backend WHPX (aceleración por hardware, Hito M11)
```powershell
# Ejecuta un guest mínimo (HLT + IO a COM1) en WHPX si el host lo soporta; si no, reporta SKIP y CapsuleOS usa backend software
.\target\release\capsule.exe test-whpx
```

### Verificar firmware BIOS y boot desde disco (Hito M12.1)
```powershell
# Usa BIOS stub integrado determinista (sin archivos externos): banner COM1, carga LBA0 a 0x7C00 y ejecuta boot sector
.\target\release\capsule.exe test-firmware

# Experimental: probar una imagen BIOS externa (p.ej. SeaBIOS). Solo se considera soportada con caso reproducible validado.
.\target\release\capsule.exe test-firmware .\ruta\bios.bin
```

### Verificar CD-ROM/ISO ATAPI y El Torito (Hito M12.2)
```powershell
# Self-test determinista: genera ISO El Torito no-emulation, parsea catalogo y ejecuta boot sector
.\target\release\capsule.exe test-cdrom

# Opcional: cargar una ISO externa (experimental/best-effort)
.\target\release\capsule.exe test-cdrom .\ruta\imagen.iso
```

### Verificar UEFI/OVMF plumbing (Hito M12.3, condicionado)
```powershell
# Self-test determinista: ROM UEFI stub 64KiB, reset 0xFFFF0, HLT+marker
.\target\release\capsule.exe test-uefi

# Opcional: mapear imagen OVMF externa (best-effort, NO SOPORTADO sin caso reproducible validado)
.\target\release\capsule.exe test-uefi .\ruta\OVMF.fd
```
Valida solo mapeo ROM/reset/ejecución mínima determinista. **No** valida OVMF completo, servicios UEFI, GOP, NVRAM, Secure Boot, ACPI, ni boot de OS reales.

### Verificar Linux Direct Boot / Ubuntu Server slice (Hito M13, condicionado)
```powershell
# Ruta sintética determinista de Linux direct boot
.\target\release\capsule.exe test-linux-boot

# Slice M13 condicionado (sin args = sintético; con args = best-effort externo)
.\target\release\capsule.exe test-ubuntu-server
.\target\release\capsule.exe test-ubuntu-server .\vmlinuz .\initrd.img "console=ttyS0"
```
**No** declara Ubuntu Server/Minimal soportado. Una imagen externa solo prueba carga/ejecución best-effort; soporte real requiere caso reproducible validado y bloqueadores de CPU/MMIO/IRQ/timers/dispositivos/firmware resueltos.

### Verificar Linux GUI / Ubuntu Desktop slice (Hito M14, condicionado)
```powershell
# Slice GUI condicionado: VBE framebuffer + PS/2 input + Linux boot sintético
.\target\release\capsule.exe test-linux-gui

# Slice M14 condicionado (sin args = sintético; con args = best-effort externo)
.\target\release\capsule.exe test-ubuntu-desktop
.\target\release\capsule.exe test-ubuntu-desktop .\vmlinuz .\initrd.img "console=ttyS0 video=800x600-32"
```
**No** declara Ubuntu Desktop/X11/Wayland soportado. Valida plumbing GUI existente, no desktop Linux real.

### Ejecutar la batería de pruebas automatizadas
```powershell
cargo test --workspace
```

---

## Documentación Técnica

Para conocer más detalles sobre la arquitectura interna y los desafíos técnicos:
- [Arquitectura de Subsistemas (docs/ARCHITECTURE.md)](docs/ARCHITECTURE.md)
- [Hoja de Ruta Completa (docs/ROADMAP.md)](docs/ROADMAP.md)
- [Matriz de Compatibilidad (docs/COMPATIBILITY.md)](docs/COMPATIBILITY.md)
- [Limitaciones Técnicas y Retos de Diseño (docs/TECHNICAL_LIMITATIONS.md)](docs/TECHNICAL_LIMITATIONS.md)

---

## 📄 Licencia

Este proyecto está bajo la licencia MIT o Apache-2.0.
