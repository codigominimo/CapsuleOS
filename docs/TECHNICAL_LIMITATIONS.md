# Limitaciones Técnicas y Retos de Diseño

Este documento expone las restricciones físicas, lógicas y arquitectónicas inherentes al diseño de un VMM autocontenido dentro de ejecutables portables de Windows.

---

## 1. Empaquetado Monolítico en Archivo `.exe` (Single-EXE)

### 1.1. Bloqueo de Archivos en Ejecución en Windows
* **Problema**: En sistemas operativos Windows, el archivo binario `.exe` en ejecución es bloqueado por el kernel (`ntoskrnl.exe`) con flags que impiden la modificación directa de sus bytes mientras exista un proceso abierto asociado a la imagen de sección (`PAGE_EXECUTE`).
* **Impacto en persistencia**: Un ejecutable `Ubuntu.exe` no puede escribir directamente dentro de su propio archivo en disco mientras está corriendo.
* **Solución arquitectónica**:
  1. *Fase inicial*: Utilizar un archivo satélite o subdirectorio local de persistencia (ej. `Ubuntu.exe.data` o `%LOCALAPPDATA%\CapsuleOS\<hash>\overlay.cow`).
  2. *Fase avanzada (Single-File persistente)*: Al cerrar la máquina virtual o mediante un proceso auxiliar de commit al apagar (`capsule-persist-worker`), transferir los bloques modificados del overlay hacia el tráiler del archivo ejecutable.

### 1.2. Límites de Tamaño en Formato PE (Portable Executable)
* **Problema**: Las cabeceras estándar PE32+ de 64 bits admiten tamaños de archivo grandes, pero imágenes completas de sistemas operativos (ej. Ubuntu Desktop: ~4 GB, Windows 11: ~6 GB) pueden generar problemas con antivirus y tiempos de carga.
* **Solución**:
  * Empleo de compresión de bloques (Zstandard / LZ4) dentro del bundle con descompresión bajo demanda (on-demand streaming reading), evitando cargar gigabytes innecesarios en la memoria RAM del host al arrancar.

### 1.3. Heurísticas de Antivirus (Windows Defender / EDRs)
* **Problema**: Ejecutables de Windows que contienen datos arbitrarios anexados tras la sección final (Overlay PE) o que asignan grandes bloques de memoria ejecutable (`PAGE_EXECUTE_READWRITE`) pueden activar falsos positivos en motores heurísticos.
* **Solución**:
  * Estructurar el payload como recurso Win32 estándar (`RT_RCDATA`) o con un tráiler firmado digitalmente con metadatos explícitos.
  * Para WHPX no se requiere memoria ejecutable en el proceso host, ya que la memoria física del guest se mapea en el hipervisor vía `WHvMapGpaRange` sobre memoria `PAGE_READWRITE`.

---

## 2. Virtualización de Hardware vs. Emulación por Software

### 2.1. Requisitos y Permisos de WHPX (M11: backend inicial)
* **Requisito**: Windows Hypervisor Platform requiere que el procesador cuente con soporte de virtualización por hardware (Intel VT-x o AMD-V) habilitado en el firmware UEFI/BIOS del equipo host.
* **Función de Windows**: La característica opcional `HypervisorPlatform` (Windows Hypervisor Platform) debe estar activa. `capsule-core::detect_whpx()` y `capsule-whpx` cargan `WinHvPlatform.dll` dinámicamente; si WHPX no está disponible, `capsule.exe test-whpx` reporta SKIP y `VirtualMachine::new()` puede hacer fallback al backend software.
* **Estado M11**: `capsule-whpx` crea partición/vCPU, mapea la GPA del guest sobre `GuestMemory` del host y atiende VM-exits básicos de E/S por puerto para integrarse con los dispositivos PIO existentes. No se usa MMIO ni `WHvEmulator`/APIC virtual completo en esta etapa.
* **Fallback a Emulador**: El backend software (`capsule-cpu`/`capsule-vmm`) permanece disponible como ruta por defecto/fallback cuando WHPX no puede inicializarse o no es adecuado para el flujo actual.

### 2.2. Rendimiento de la Emulación por Software
* **Limitación**: El backend de CPU software actual no implementa entrega de interrupciones arquitecturales (IDT/CLI/STI/IRET/APIC). Aunque el PIC/PIT/RTC/PCI/IDE/PS/2 reflejan IRQs y condiciones pendientes (p. ej. PIT IRQ0, teclado IRQ1, INTx PCI/IRQ11, ratón IRQ12, IDE IRQ14), la VM procesa dispositivos por sondeo (polling) en cada tick/iteración y refresca líneas de IRQ hacia el PIC.
* **Impacto**: Guests que dependan estrictamente de interrupciones temporizadas o de notificación asíncrona por IRQ pueden no funcionar correctamente hasta que se implemente inyección/entrega de interrupciones en la vCPU. Esto también aplica a señales de video (p. ej. VBlank) no disponibles y a entrada dependiente de IRQs reales (p. ej. drivers que no sondeen puertos PS/2/PIC).

### 2.3. Limitaciones del backend WHPX en M11
* **MMIO no soportado**: el modelo actual de dispositivos es PIO (puertos) y memoria guest lineal; VM-exits de acceso a memoria/MMIO no se despachan a dispositivos MMIO.
* **Sin inyección arquitectural de interrupciones**: no hay entrega de IRQ/excepciones vía IDT/CLI/STI/IRET/APIC/IOAPIC en WHPX (ni en software). Los dispositivos siguen dependiendo de sondeo/ticks y de las líneas reflejadas al PIC según el modelo existente.
* **Estado inicial de vCPU mínimo**: configuración básica orientada a pruebas planas (HLT/IO); no se implementan long mode, tablas de sistema completas, MSRs avanzados ni ruta de firmware/boot de kernels reales sobre WHPX en M11.
* **Decodificación de VM-exit IO**: la obtención de longitud de instrucción y contexto de salida usa offsets/heurísticas según layout disponible y un fallback de decodificación mínima desde memoria guest para avanzar RIP en instrucciones `IN/OUT` comunes.
* **Paridad parcial con software**: el backend WHPX reutiliza dispositivos/timing por tick del VMM, por lo que la fidelidad temporal y la semántica de ejecución pueden diferir del objetivo para guests complejos.

---

## 3. Dispositivos y Controladores

### 3.1. Red sin Privilegios Administrativos (M8: implementación inicial)
* **Problema**: La creación de interfaces de red virtuales en Windows (TAP/Wintun) suele requerir privilegios de Administrador.
* **Estado M8**: `capsule-network::UserModeNat` implementa una pila/NAT en modo usuario básica (ARP, DHCP básico, ICMP echo a IPs virtuales, reenvío UDP saliente mediante sockets del host con un flujo pendiente) expuesta al guest por `VirtioNetPci` (PCI legacy I/O BAR0, virtqueues RX/TX en memoria guest) operada por sondeo desde la VM.
* **Limitaciones actuales**: sin TCP y sin múltiples flujos/conexiones concurrentes completas; DNS no es resolución real completa (uso de IPs virtuales/reenvío limitado); sin MMIO/transporte moderno y sin inyección de IRQ en CPU (IDT/CLI/STI/IRET), por lo que el dispositivo depende de polling; INTx PCI se agrega como IRQ11 en PIC a nivel de plataforma.

### 3.2. Gráficos / Framebuffer (M9: implementación inicial Bochs VBE)
* **Estado M9**: Se implementa `BochsVbe` con programación por puertos DISPI (`0x01CE/0x01CF`) y framebuffer lineal (LFB) en RAM guest en `0x0040_0000`, leído por la GUI del host y bliteado a la ventana Win32 con GDI (`StretchDIBits`, DIB 32bpp top-down) en tiempo real por sondeo/timer.
* **Limitaciones actuales**: LFB modelado en RAM guest (no BAR MMIO dedicado de dispositivo PCI/VGA) para encajar en el modelo actual sin MMIO; sin VBlank/IRQ ni sincronización avanzada; compatibilidad VBE/parcial orientada a validación inicial; sin aceleración 2D/3D ni `virtio-gpu`.
* **Impacto**: Guests gráficos reales pueden requerir mayor fidelidad VBE/VGA, MMIO y temporización/IRQ para funcionar correctamente.
* **Solución**: Evolucionar a adaptador de framebuffer con MMIO/BAR adecuado, sincronización eficiente y ruta de presentación optimizada (DIBSection/Direct3D 11) para estabilidad y rendimiento.

### 3.3. Entrada PS/2 i8042 (M10: implementación inicial teclado/ratón)
* **Estado M10**: Se implementa `I8042Ps2Controller` (0x60/0x64) con cola de salida, comandos básicos de controlador/dispositivos, teclado PS/2 (scancodes Set 1) y ratón PS/2 (modo stream, paquetes de 3 bytes). La GUI Win32 traduce eventos de teclado/ratón a inyecciones PS/2 en la VM; IRQ1/IRQ12 se reflejan al PIC por sondeo.
* **Limitaciones actuales**: sin inyección de IRQ en CPU (IDT/CLI/STI/IRET/APIC), por lo que drivers guest dependientes de interrupciones reales pueden no funcionar; mapeo de teclado parcial (sin layouts/IME completos, sin todas las teclas extendidas), sin rueda ni botones extendidos, captura de ratón básica y sin dispositivos USB/HID/touch.
* **Impacto**: Adecuado para validación y guests/bootloaders que sondeen puertos PS/2; compatibilidad limitada con sistemas que requieran mayor fidelidad de input o IRQs hardware entregadas por CPU.

### 3.4. Firmware BIOS/UEFI/CD-ROM (M12: slices acotados, M12.3 condicionado)
* **Estado M12.1**: `capsule-memory` soporta ROM mapeada (writes ignoradas). `capsule-vmm` puede mapear una imagen BIOS (externa opcional) o un stub BIOS integrado determinista 64 KiB, configurar reset minimo y arrancar desde disco IDE primario en backend software. El stub imprime banner por COM1, lee LBA0 via ATA PIO a `0x7C00` y transfiere control al boot sector.
* **Estado M12.2**: ATAPI PIO mínimo en IDE secundario maestro (0x170/0x376, IRQ15), sectores 2048 bytes, `IDENTIFY PACKET DEVICE`, `PACKET` y SCSI básico (`TEST UNIT READY`, `REQUEST SENSE`, `INQUIRY`, `READ CAPACITY(10)`, `READ(10/12)`, `MODE SENSE(10)` stub). `boot_from_cdrom_secondary_master` parsea El Torito (Boot Record Volume, catalog, validation entry, initial/default x86 no-emulation), carga imagen boot en `0x7C00` y salta con DL=0xE0; fallback LBA0. `test-cdrom` valida caso determinista.
* **Estado M12.3 (condicionado)**: `map_firmware_rom`/`boot_firmware_uefi_image` mapean imagen UEFI/OVMF en ROM y configuran reset mínimo determinista. `test-uefi` valida plumbing con stub ROM determinista (HLT+marker). OVMF externo es best-effort: **no declarado soportado** sin caso reproducible validado.
* **Limitaciones actuales**: no es SeaBIOS/OVMF completo; sin modo real/segmentacion, IVT/servicios INT BIOS, INT 13h/extensions CD, A20 gate, PCI BIOS, DMA/UDMA, entrega arquitectural de IRQ en CPU, ACPI/SMBIOS completas. ATAPI/El Torito mínimo (no-emulation, sin ISO9660 completo, sin media change). UEFI sin servicios/runtime/GOP/NVRAM/Secure Boot.
* **Bloqueadores M12.3**: MMIO, IRQ/APIC/IOAPIC, CPU x86_64 completa (long mode/paginación/excepciones/MSRs/SMM), ACPI/HPET/timers, dispositivos firmware/PCI completos.

### 3.5. Ubuntu Minimal/Server (M13, slice condicionado/bloqueado)
* **Estado**: CapsuleOS tiene ruta de Linux direct boot (`bzImage`, setup header, zero-page/boot_params, cmdline, initrd, pagetables identity) usada por tests sintéticos y `test-linux-boot`. `test-ubuntu-server` expone un slice condicionado para imágenes externas.
* **No soportado**: Ubuntu Server/Minimal real **no está soportado/validado**. No existe caso reproducible de boot completo de Ubuntu en el runtime actual.
* **Bloqueadores críticos**: MMIO, entrega arquitectural de interrupciones (IDT/PIC/APIC/IOAPIC/CLI/STI/IRET), CPU x86_64 incompleta (long mode/paginación/excepciones/MSRs/faults), timers/APIC/HPET/ACPI, dispositivos y drivers necesarios para Linux real (virtio/PCI completos, almacenamiento/red/console compatibles), firmware/UEFI/bootloader real.
* **Criterio de soporte futuro**: solo declarar Ubuntu soportado con imagen/artefactos identificados, comandos reproducibles, logs/serial capturados y validación automatizada en CI/host compatible.
