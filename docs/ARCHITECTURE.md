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
  * Definición del trait unificado `VmBackend`:
    ```rust
    pub trait VmBackend {
        fn create_vm(&mut self, config: &VmConfig) -> Result<(), VmmError>;
        fn create_vcpu(&mut self, id: usize) -> Result<Box<dyn VirtualCpu>, VmmError>;
        fn map_memory(&mut self, gpa: u64, size: usize, hva: *mut u8, flags: MemoryFlags) -> Result<(), VmmError>;
        fn run_vcpu(&mut self, id: usize) -> Result<VcpuExit, VmmError>;
    }
    ```
  * Despacho de salidas de la VM (VM Exits: Port I/O, HLT, errores) y, en el backend software actual, avance por tick/sondeo de dispositivos de plataforma y PCI (PIT/RTC, IDE IRQ14, INTx PCI agregada/IRQ11, virtqueues/NAT) con acceso a memoria guest, ademas de stepping incremental para UI y utilidades de lectura/copia de framebuffer VBE.
  * Nota: MMIO y entrega/inyección de interrupciones a la CPU software (IDT/CLI/STI/IRET) pendientes.

* **`capsule-cpu`**:
  * Definición del estado de la CPU x86-64: registros de propósito general (RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8-R15), puntero de instrucción (RIP), registro de banderas (RFLAGS), registros de control (CR0, CR2, CR3, CR4, CR8), registros de segmento (CS, DS, SS, ES, FS, GS), tablas descriptores (GDTR, IDTR, LDTR, TR) y MSRs esenciales.

### 2.2. Backends de Ejecución
* **`capsule-whpx`**:
  * Backend de virtualización acelerada por hardware nativa de Windows mediante `WinHvPlatform.dll` y `WinHvEmulation.dll`.
  * Carga dinámica de la biblioteca para garantizar que el binario arranque incluso en hosts sin WHPX.
  * Creación de partición WHPX, mapeo de memoria EPT/NPT, gestión de interrupciones virtuales (APIC/IOAPIC) y ejecución vCPU.

* **`capsule-emulator`**:
  * Backend de emulación puramente por software para CPUs x86_64.
  * Permite ejecutar guests en máquinas donde WHPX no está habilitado o no está soportado.
  * Incluye decodificador x86, intérprete de instrucciones, traducción de direcciones virtuales a físicas (MMU x86-64 con soporte de páginas de 4KB, 2MB y 1GB en modo protegido y modo largo) y manejo de interrupciones/excepciones.

### 2.3. Memoria y Dispositivos
* **`capsule-memory`**:
  * Asignación y gestión de memoria física del guest (`GuestMemory`).
  * Asignación de páginas en memoria virtual del host alineadas a 4KB / 2MB.
  * Mapeo de regiones RAM, MMIO y ROM reservada.

* **`capsule-devices`**:
  * Enrutador de buses: Port I/O (espacio de direcciones de 16 bits `in`/`out`).
  * Dispositivos de plataforma implementados (M4-M9):
    * Puerto serie UART 16550 (COM1 en 0x3F8) para salida de consola y depuración del kernel.
    * Controlador de interrupciones PIC 8259 dual (maestro/esclavo) con ICW/OCW, IMR/ISR/IRR, cascada y EOI.
    * Temporizador PIT 8254 (canales 0-2, modos básicos) y RTC/CMOS MC146818 mínimo (0x70/0x71).
    * Bus PCI extensible (mecanismo #1, `0xCF8/0xCFC`) con enrutamiento de config/E/S, registro de funciones PCI y sondeo de dispositivos con acceso a memoria guest.
    * NIC `VirtioNetPci` transitional (legacy PCI I/O BAR0, split virtqueues RX/TX) integrada al bus PCI; sin MMIO.
    * Adaptador grafico `BochsVbe` (puertos DISPI `0x01CE/0x01CF`) con LFB en RAM guest (`0x0040_0000`) para render host por sondeo.
  * Pendiente: MMIO, inyección de interrupciones a la CPU software (IDT/CLI/STI/IRET), APIC local/IO-APIC y mayor fidelidad temporal.

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
* **`capsule-input`**:
  * Controlador de teclado y ratón PS/2 (puertos 0x60/0x64).
  * Conversión de mensajes de entrada Win32 (`WM_KEYDOWN`, `WM_KEYUP`, `WM_MOUSEMOVE`, etc.) a scan codes PS/2 (Scan Code Set 1 y 2).

* **`capsule-network`**:
  * Pila de red en modo usuario (`UserModeNat`, M8) sin TAP/Wintun ni privilegios de administrador:
    * Respuestas ARP para gateway/DNS virtual, DHCP básico (OFFER/ACK), ICMP echo a IPs virtuales.
    * Reenvío UDP saliente mediante sockets del host con retorno de respuestas al guest (flujo UDP pendiente limitado).
  * Integrada con `capsule-devices::VirtioNetPci`; procesamiento por sondeo desde el bucle de la VM con acceso a memoria guest.
  * Pendiente: TCP, múltiples flujos/conexiones concurrentes, NAT/traducción completa, DNS real y mayor fidelidad de pila.

### 2.4. Arranque y Firmware
* **`capsule-firmware`**:
  * Soporte de firmware BIOS heredado (SeaBIOS embebido o tabla de reset mínima).
  * Soporte UEFI mediante OVMF (Open Virtual Machine Firmware de EDK2).
  * Tablas ACPI estáticas (RSDP, RSDT, XSDT, FADT, MADT, DSDT) necesarias para que los sistemas operativos modernos reconozcan múltiples núcleos y dispositivos PCI.

* **`capsule-boot`**:
  * Cargador directo de kernel Linux (protocolo de arranque x86 de Linux bzImage).
  * Carga de `vmlinuz` + `initramfs` + `boot_params` (zero-page) directo en memoria RAM del guest, permitiendo arranque instantáneo de Linux en milisegundos sin pasar por la BIOS.

### 2.5. Empaquetado y Persistencia
* **`capsule-persistence`** (actualmente implementado en `capsule-storage` como `CowOverlayDisk`, M6):
  * Capa de disco Copy-on-Write (COW) con bitmap por sector y overlay persistente en archivo.
  * Mantiene la imagen base intacta y almacena las modificaciones en un archivo diferencial/overlay (integrable al bundle/cápsula en hitos posteriores).

* **`capsule-bundle`**:
  * Especificación y parser del contenedor monolitico Capsule:
    * `Header`: Identificador mágico `CAPSULE\0`, versión, offsets.
    * `Manifest`: Configuración de vCPU, RAM, firmware, discos y red.
    * `Payload`: Firmware, kernel, ramdisk o imagen de disco comprimida (zstd/lz4).
  * Detección de ejecución como cápsula empaquetada o como herramienta de desarrollo.

### 2.6. Interfaz de Usuario y CLI
* **`capsule-gui`**:
  * Ventana nativa Win32.
  * Bucle de ejecución por pasos de VM (`step_once/run_steps`) y renderizado en tiempo real del framebuffer guest mediante GDI/DIB (`StretchDIBits`) cuando VBE LFB está activo (M9), con fallback a panel de diagnósticos.
  * Manejo básico de ventana/redimensionado y cierre (input de teclado/ratón hacia el guest pendiente, M10).
* **`capsule-cli`**:
  * Entrada por línea de comandos para control, inspección, arranque headless e integración con scripts.

* **`capsule-builder`**:
  * Herramienta de compilación para usuarios que desean generar sus propios ejecutables `.exe` a partir de ISOs o VHDs.
