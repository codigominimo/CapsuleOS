# Arquitectura de CapsuleOS

CapsuleOS es una plataforma de ejecución y empaquetado para Windows que permite empaquetar sistemas operativos completos dentro de ejecutables portables `.exe`. El usuario final simplemente ejecuta el binario (ej. `Ubuntu.exe`, `Windows.exe`) y obtiene una máquina virtual en ejecución sin necesidad de preinstalar hipervisores externos (VirtualBox, VMware, QEMU).

---

## 1. Visión Arquitectónica General

CapsuleOS consta de dos componentes conceptuales principales:
1. **Capsule Builder (`tools/capsule-builder`)**: Herramienta de construcción que ingesta imágenes de disco o instaladores (`.iso`, `.img`, `.vhd`, `.vhdx`), analiza el sistema operativo, genera los metadatos y ensambla un runtime portable autocontenido dentro de un archivo ejecutable PE de Windows (`.exe`).
2. **Capsule Runtime (`capsule.exe` / crates modulares)**: El ejecutable autónomo final que integra el monitor de máquinas virtuales (VMM), el backend de virtualización por hardware (WHPX) o emulación por software, los dispositivos virtuales y la interfaz gráfica/consola en una ventana de Windows.

```
+--------------------------------------------------------------------------+
|                              Capsule.exe                                 |
|                                                                          |
|  +--------------------+  +--------------------------------------------+  |
|  | Capsule Bundle     |  | Capsule Runtime / GUI Window (Win32/GDI)   |  |
|  | - Manifest (JSON)  |  +--------------------------------------------+  |
|  | - Firmware (OVMF)  |  | Capsule VMM Abstraction Layer (VmBackend)  |  |
|  | - Kernel / Initrd  |  +---------------------+----------------------+  |
|  | - Base Disk Image  |                        |                         |
|  +--------------------+         +--------------+--------------+          |
|                                 |                             |          |
|                                 v                             v          |
|                     +-----------------------+     +-------------------+  |
|                     | capsule-whpx (WHPX)   |     | capsule-emulator  |  |
|                     | Hardware Accel (VT-x) |     | Software CPU/MMU  |  |
|                     +-----------------------+     +-------------------+  |
|                                 |                             |          |
|                                 +--------------+--------------+          |
|                                                |                         |
|  +---------------------------------------------v----------------------+  |
|  | Virtual Platform & Devices                                         |  |
|  | - Memory Bus / Guest Physical Memory (capsule-memory)              |  |
|  | - I/O Bus & PCI Bus (capsule-devices)                              |  |
|  | - Storage Controller: virtio-blk / IDE (capsule-storage)           |  |
|  | - Display: Framebuffer / Bochs VBE (capsule-display)               |  |
|  | - Input: PS/2 Keyboard & Mouse (capsule-input)                     |  |
|  | - Network: virtio-net / User-mode NAT (capsule-network)            |  |
|  | - Persistence: Copy-On-Write / Overlay (capsule-persistence)       |  |
|  +--------------------------------------------------------------------+  |
|                                                |                         |
|                                                v                         |
|                                    +-----------------------+             |
|                                    | Guest Operating System|             |
|                                    | (Linux / Windows/BSD) |             |
|                                    +-----------------------+             |
+--------------------------------------------------------------------------+
```

---

## 2. Desglose Modular de Crates (Cargo Workspace)

El espacio de trabajo se organiza en crates independientes y con responsabilidades desacopladas:

### 2.1. Núcleo y Abstracciones
* **`capsule-core`**:
  * Tipos de datos fundamentales (`GuestAddress`, `HostAddress`, tamaños, constantes).
  * Detección del entorno host (versión de Windows, CPU x86_64, número de núcleos lógicos, memoria física disponible).
  * Comprobación dinámica de soporte para virtualización por hardware (WHPX / Hyper-V).
  * Manejo de errores y logging estructurado.

* **`capsule-vmm`**:
  * Orquestación de la máquina virtual.
  * Selección de backend de ejecución en M11 (`VmBackendKind::{Software,Whpx}`) con API de acceso a registros unificada en `VirtualMachine`; `new()` intenta WHPX solo si se solicita y hace fallback a software, `new_whpx()` fuerza WHPX.
  * Traits de abstracción (`VmBackend`/`VirtualCpu`) definidos para evolución futura; la integración M11 usa un backend interno concreto (`Software`/`Whpx`) para preservar el comportamiento/API existentes.
  * Despacho de salidas de la VM (VM Exits/software exits: Port I/O, HLT, errores). En software y en WHPX (IO exits) se reutiliza el mismo despacho de dispositivos PIO y tick/sondeo de plataforma/PCI (PIT/RTC, IDE IRQ14, INTx PCI agregada/IRQ11, PS/2 IRQ1/IRQ12, virtqueues/NAT), además de stepping incremental para UI, utilidades de lectura/copia de framebuffer VBE e inyección de eventos de entrada PS/2 desde la GUI.
  * Nota: MMIO y entrega/inyección arquitectural de interrupciones a la CPU (IDT/CLI/STI/IRET/APIC) pendientes en ambos caminos.

* **`capsule-cpu`**:
  * Definición del estado de la CPU x86-64: registros de propósito general (RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8-R15), puntero de instrucción (RIP), registro de banderas (RFLAGS), registros de control (CR0, CR2, CR3, CR4, CR8), registros de segmento (CS, DS, SS, ES, FS, GS), tablas descriptores (GDTR, IDTR, LDTR, TR) y MSRs esenciales.

### 2.2. Backends de Ejecución
* **`capsule-whpx`** (implementado en M11, alcance mínimo):
  * Backend de virtualización acelerada por hardware nativa de Windows mediante carga dinámica de `WinHvPlatform.dll` (sin dependencia dura en hosts sin WHPX).
  * Creación de partición WHPX, creación de vCPU, mapeo de la memoria física guest (`GuestMemory`) como GPA y ejecución por `WHvRunVirtualProcessor`.
  * Sincronización de registros generales/RIP/RFLAGS y manejo de VM-exits básicos, principalmente E/S por puerto (IN/OUT), traducidos al bus de dispositivos emulados del VMM.
  * Pendiente: MMIO, emulación/instrucciones adicionales según exits, APIC/IOAPIC/interrupciones virtuales e inyección arquitectural, soporte de modos/estado de CPU completo para firmware/kernels reales.

* **`capsule-emulator` / backend software** (integrado actualmente en `capsule-cpu` + `capsule-vmm`):
  * Ejecución puramente por software para x86-64 usada como backend por defecto y como fallback cuando WHPX no está disponible.
  * Decodificador/intérprete de instrucciones, registros y manejo de salidas de E/S/HLT sobre `GuestMemory`.
  * Limitado: sin entrega arquitectural de interrupciones (IDT/CLI/STI/IRET/APIC) y sin MMU/modos completos equivalentes a un guest moderno.

### 2.3. Memoria y Dispositivos
* **`capsule-memory`**:
  * Asignación y gestión de memoria física del guest (`GuestMemory`).
  * Asignación de páginas en memoria virtual del host alineadas a 4KB / 2MB.
  * Mapeo de regiones RAM, MMIO y ROM reservada.

* **`capsule-devices`**:
  * Enrutador de buses: Port I/O (espacio de direcciones de 16 bits `in`/`out`).
  * Dispositivos de plataforma implementados (M4-M10):
    * Puerto serie UART 16550 (COM1 en 0x3F8) para salida de consola y depuración del kernel.
    * Controlador de interrupciones PIC 8259 dual (maestro/esclavo) con ICW/OCW, IMR/ISR/IRR, cascada y EOI.
    * Temporizador PIT 8254 (canales 0-2, modos básicos) y RTC/CMOS MC146818 mínimo (0x70/0x71).
    * Controlador PS/2 i8042 (puertos `0x60/0x64`) con teclado (scancodes Set 1) y ratón (paquetes PS/2 de 3 bytes), con IRQ1/IRQ12 reflejadas al PIC por sondeo según datos pendientes.
    * Bus PCI extensible (mecanismo #1, `0xCF8/0xCFC`) con enrutamiento de config/E/S, registro de funciones PCI y sondeo de dispositivos con acceso a memoria guest.
    * NIC `VirtioNetPci` transitional (legacy PCI I/O BAR0, split virtqueues RX/TX) integrada al bus PCI; sin MMIO.
    * Adaptador gráfico `BochsVbe` (puertos DISPI `0x01CE/0x01CF`) con LFB en RAM guest (`0x0040_0000`) para render host por sondeo.
  * Pendiente: MMIO, inyección de interrupciones a la CPU software (IDT/CLI/STI/IRET/APIC), APIC local/IO-APIC y mayor fidelidad temporal.

* **`capsule-storage`**:
  * Abstracción de medios de almacenamiento (`BlockDevice`) y backends implementados: RAM (`MemoryDisk`), RAW, VHD fixed (512-byte) y overlay COW (`CowOverlayDisk`, bitmap por sector, base inmutable).
  * Formatos/funciones planeadas: VHD dynamic, VHDX, QCOW2 y `virtio-blk`.
  * Controladores de almacenamiento virtual:
    * IDE / ATA PIO primario (maestro, LBA28) implementado para M5/M6 (sin DMA). IRQ14 se refleja a nivel de PIC según estado IDE (`irq_pending && !nIEN`), pero la CPU software aún no inyecta interrupciones.
    * `virtio-blk` planeado para alto rendimiento en guests modernos.

* **`capsule-display`** (funcionalidad integrada actualmente en `capsule-devices` + `capsule-gui`, crate dedicado pendiente):
  * Framebuffer lineal en memoria guest (LFB) expuesto por `BochsVbe` (M9), con lectura por sondeo desde `GuestMemory`.
  * Emulación de adaptadores gráficos:
    * Bochs VBE DISPI (M9, validación inicial 32bpp/LFB en RAM guest).
    * `virtio-gpu` (futuro).
  * Sincronización de refresco con la ventana mediante timer GUI y blit Win32 (`StretchDIBits`).

* **`capsule-input`** (integrado actualmente en `capsule-devices::I8042Ps2Controller` + `capsule-gui`, M10; crate dedicado pendiente):
  * Controlador PS/2 i8042 (0x60/0x64) con teclado (Set 1) y ratón (paquetes de 3 bytes) e IRQ1/IRQ12 reflejadas al PIC por sondeo.
  * Conversión de eventos Win32 (`WM_KEYDOWN/UP`, `WM_SYSKEY*`, `WM_MOUSEMOVE`, `WM_*BUTTON*`) a scancodes/paquetes PS/2 inyectados en la VM. Pendiente: mapeo de teclado más completo, rueda/botones extendidos y USB/HID.

* **`capsule-network`**:
  * Pila de red en modo usuario (`UserModeNat`, M8) sin TAP/Wintun ni privilegios de administrador:
    * Respuestas ARP para gateway/DNS virtual, DHCP básico (OFFER/ACK), ICMP echo a IPs virtuales.
    * Reenvío UDP saliente mediante sockets del host con retorno de respuestas al guest (flujo UDP pendiente limitado).
  * Integrada con `capsule-devices::VirtioNetPci`; procesamiento por sondeo desde el bucle de la VM con acceso a memoria guest.
  * Pendiente: TCP, múltiples flujos/conexiones concurrentes, NAT/traducción completa, DNS real y mayor fidelidad de pila.

### 2.4. Arranque y Firmware
* **`capsule-firmware`** (planeado como crate dedicado; M12.1/M12.2 implementados incrementalmente en `capsule-vmm`/`capsule-memory`/`capsule-devices`):
  * M12.1: mapeo ROM BIOS en GPA (`GuestMemory::map_rom`), reset vector/configuracion minima y boot desde disco IDE primario. Stub BIOS integrado determinista 64 KiB para backend software (COM1 banner + ATA PIO LBA0 -> 0x7C00 + JMP boot sector). `VmConfig.firmware_bios_path` permite cargar imagen BIOS externa (experimental/best-effort).
  * M12.2: ATAPI PIO mínimo en IDE secundario maestro (2048-byte sectors, IDENTIFY PACKET DEVICE, PACKET/READ(10), IRQ15). `boot_from_cdrom_secondary_master` parsea El Torito Boot Record Volume/catalog/default x86 no-emulation y carga imagen boot en 0x7C00 (fallback LBA0). CLI `test-cdrom` valida ISO determinista; ISO externa experimental.
  * M12.3 (condicionado, no soporte completo): `VmConfig.firmware_uefi_path`, `map_firmware_rom`/`boot_firmware_uefi_image` mapean ROM y configuran reset mínimo. `test-uefi` valida plumbing determinista; OVMF externo es best-effort/no soportado sin caso reproducible validado. Bloqueadores: MMIO, IRQ/APIC, CPU/modos/excepciones/MSRs, timers/ACPI, dispositivos firmware/GOP/NVRAM.
  * Tablas ACPI/firmware completo siguen pendientes.

* **`capsule-boot`**:
  * M13 (condicionado/bloqueado): plumbing `linux_kernel_path/initrd_path/cmdline` en `VmConfig`; CLIs `test-linux-boot`/`test-ubuntu-server` validan ruta sintética y permiten best-effort externo. **Ubuntu Server no soportado** sin caso reproducible validado.
  * M14 (condicionado/bloqueado): CLIs `test-linux-gui`/`test-ubuntu-desktop` componen framebuffer VBE + input PS/2 + Linux direct boot para validar plumbing GUI determinista/best-effort. **Ubuntu Desktop/X11/Wayland no soportado** sin caso reproducible validado.
  * Cargador directo de kernel Linux (protocolo de arranque x86 de Linux bzImage).
  * Carga de `vmlinuz` + `initramfs` + `boot_params` (zero-page) directo en memoria RAM del guest, permitiendo arranque instantáneo de Linux en milisegundos sin pasar por la BIOS.

### 2.5. Empaquetado y Persistencia
* **`capsule-persistence`** (actualmente implementado en `capsule-storage` como `CowOverlayDisk`, M6):
  * Capa de disco Copy-on-Write (COW) con bitmap por sector y overlay persistente en archivo.
  * Mantiene la imagen base intacta y almacena las modificaciones en un archivo diferencial/overlay (integrable al bundle/cápsula en hitos posteriores).

* **`capsule-bundle`**:
  * Especificación y parser del contenedor monolítico Capsule:
    * `Header`: Identificador mágico `CAPSULE\0`, versión, offsets.
    * `Manifest`: Configuración de vCPU, RAM, firmware, discos y red.
    * `Payload`: Firmware, kernel, ramdisk o imagen de disco comprimida (zstd/lz4).
  * Detección de ejecución como cápsula empaquetada o como herramienta de desarrollo.

### 2.6. Interfaz de Usuario y CLI
* **`capsule-gui`**:
  * Ventana nativa Win32.
  * Bucle de ejecución por pasos de VM (`step_once/run_steps`) y renderizado en tiempo real del framebuffer guest mediante GDI/DIB (`StretchDIBits`) cuando VBE LFB está activo (M9), con fallback a panel de diagnósticos.
  * Captura y traducción de entrada Win32 (teclado/ratón) a eventos PS/2 del guest (M10), con captura/liberación de ratón en ventana (clic/Ctrl+Esc). Pendiente: mayor fidelidad de input y mapeos extendidos.
* **`capsule-cli`**:
  * Entrada por línea de comandos para control, inspección, arranque headless e integración con scripts.

* **`capsule-builder`**:
  * Herramienta de compilación para usuarios que desean generar sus propios ejecutables `.exe` a partir de ISOs o VHDs.
