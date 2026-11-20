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
  * Despacho de salidas de la VM (VM Exits: Port I/O, MMIO, HLT, interrupciones, excepciones).

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
  * Enrutador de buses: Port I/O (espacio de direcciones de 16 bits `in`/`out`) y MMIO.
  * Dispositivos de plataforma estándar de PC:
    * Puerto serie UART 16550 (COM1 en 0x3F8) para salida de consola y depuración del kernel.
    * Temporizador PIT 8254 y CMOS/RTC (puertos 0x70/0x71).
    * Controlador de interrupciones PIC 8259 heredado y APIC local/IO-APIC.
    * Bus PCI sintético para descubrimiento de dispositivos periféricos.

* **`capsule-storage`**:
  * Abstracción de medios de almacenamiento:
    * Formatos soportados: RAW, VHD (Fixed y Dynamic), VHDX, QCOW2.
  * Controladores de almacenamiento virtual:
    * IDE / ATA heredado (para BIOS inicial / compatibilidad máxima).
    * `virtio-blk` para alto rendimiento en Linux y Windows modernos con drivers VirtIO.

* **`capsule-display`**:
  * Framebuffer gráfico lineal en memoria (formato BGRA32 / BGR24).
  * Emulación de adaptadores gráficos:
    * Dispositivo VGA simple / extensiones Bochs VBE (utilizado universalmente por Linux y Windows).
    * `virtio-gpu` (futuro).
  * Sincronización de refresco con la ventana de la aplicación.

* **`capsule-input`**:
  * Controlador de teclado y ratón PS/2 (puertos 0x60/0x64).
  * Conversión de mensajes de entrada Win32 (`WM_KEYDOWN`, `WM_KEYUP`, `WM_MOUSEMOVE`, etc.) a scan codes PS/2 (Scan Code Set 1 y 2).

* **`capsule-network`**:
  * Dispositivo de red `virtio-net`.
  * Pila de red en modo usuario (User-mode NAT) para proporcionar conectividad a Internet al guest sin necesidad de controladores TAP/Wintun ni privilegios de administrador.

### 2.4. Arranque y Firmware
* **`capsule-firmware`**:
  * Soporte de firmware BIOS heredado (SeaBIOS embebido o tabla de reset mínima).
  * Soporte UEFI mediante OVMF (Open Virtual Machine Firmware de EDK2).
  * Tablas ACPI estáticas (RSDP, RSDT, XSDT, FADT, MADT, DSDT) necesarias para que los sistemas operativos modernos reconozcan múltiples núcleos y dispositivos PCI.

* **`capsule-boot`**:
  * Cargador directo de kernel Linux (protocolo de arranque x86 de Linux bzImage).
  * Carga de `vmlinuz` + `initramfs` + `boot_params` (zero-page) directo en memoria RAM del guest, permitiendo arranque instantáneo de Linux en milisegundos sin pasar por la BIOS.

### 2.5. Empaquetado y Persistencia
* **`capsule-persistence`**:
  * Capa de disco Copy-On-Write (COW).
  * Mantiene la imagen base intacta dentro del bundle y almacena las modificaciones en un archivo diferencial (`.data` o payload persistente).

* **`capsule-bundle`**:
  * Especificación y parser del contenedor monolitico Capsule:
    * `Header`: Identificador mágico `CAPSULE\0`, versión, offsets.
    * `Manifest`: Configuración de vCPU, RAM, firmware, discos y red.
    * `Payload`: Firmware, kernel, ramdisk o imagen de disco comprimida (zstd/lz4).
  * Detección de ejecución como cápsula empaquetada o como herramienta de desarrollo.

### 2.6. Interfaz de Usuario y CLI
* **`capsule-gui`**:
  * Ventana nativa Win32 optimizada para latencia cero.
  * Renderizado del framebuffer guest mediante GDI `BitBlt` o DirectX / DirectComposition.
  * Manejo de redimensionado, foco de entrada, captura de ratón y atajos de liberación.

* **`capsule-cli`**:
  * Entrada por línea de comandos para control, inspección, arranque headless e integración con scripts.

* **`capsule-builder`**:
  * Herramienta de compilación para usuarios que desean generar sus propios ejecutables `.exe` a partir de ISOs o VHDs.
