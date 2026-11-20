# CapsuleOS

> **Sistemas operativos completos dentro de una c├ípsula ejecutable portable `.exe` para Windows.**

CapsuleOS es un entorno de ejecuci├│n y empaquetado para Windows escrito en Rust que permite ejecutar sistemas operativos completos (Ubuntu, Debian, Alpine, FreeBSD, Windows, etc.) como si fueran aplicaciones portables independientes de Windows.

```text
Ubuntu.exe   ÔåÆ arranca Ubuntu
Debian.exe   ÔåÆ arranca Debian
Alpine.exe   ÔåÆ arranca Alpine
FreeBSD.exe  ÔåÆ arranca FreeBSD
Windows.exe  ÔåÆ arranca Windows
```

El usuario final simplemente hace doble clic sobre el `.exe` y obtiene una ventana con el sistema operativo real ejecut├índose dentro de una m├íquina virtual. **No es necesario instalar previamente VirtualBox, VMware ni QEMU.** CapsuleOS proporciona su propio runtime.

---

## ­ƒÅø´©Å Diferenciaci├│n Clave

| Entorno Tradicional (VirtualBox / VMware) | CapsuleOS |
|---|---|
| Hipervisor externo pesado instalado en el SO | Runtime aut├│nomo integrado en el propio `.exe` |
| Asistentes de configuraci├│n manual de VMs | Cero configuraci├│n: doble clic y arranque directo |
| Archivos dispersos (`.vdi`, `.vmx`, `.vbox`, etc.) | C├ípsula autocontenida (`Ubuntu.exe`) |
| Dependencia de drivers de kernel de terceros | Uso de APIs nativas de Windows (**WHPX**) o emulador software |

---

## ÔÜí Backends de Ejecuci├│n

CapsuleOS implementa una arquitectura h├¡brida de doble motor:

```text
                  CapsuleOS
                      Ôöé
          ÔöîÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔö┤ÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÉ
          Ôöé                       Ôöé
     Hardware VM              Emulador
          Ôöé                       Ôöé
     WHPX (VT-x/SVM)        Software x86_64
          Ôöé                       Ôöé
          ÔööÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔö¼ÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÇÔöÿ
                      Ôöé
                   Guest OS
```

1. **Aceleraci├│n por Hardware (WHPX)**: Utiliza la API nativa *Windows Hypervisor Platform* (`WinHvPlatform.dll`) cuando est├í habilitada en Windows, ofreciendo rendimiento casi nativo.
2. **Emulaci├│n por Software**: Int├®rprete/emulador de CPU x86-64 puro para entornos donde la virtualizaci├│n por hardware no est├® disponible o est├® deshabilitada.

---

## ­ƒôª Estructura del Cargo Workspace

El proyecto est├í dise├▒ado de forma estrictamente modular y desacoplada:

```text
CapsuleOS/
Ôö£ÔöÇÔöÇ Cargo.toml                      # Definici├│n del Workspace
Ôö£ÔöÇÔöÇ docs/                           # Especificaciones y an├ílisis t├®cnico
Ôöé   Ôö£ÔöÇÔöÇ ARCHITECTURE.md             # Arquitectura detallada de subsistemas
Ôöé   Ôö£ÔöÇÔöÇ ROADMAP.md                  # Hoja de ruta completa de hitos (M0 a M21)
Ôöé   Ôö£ÔöÇÔöÇ COMPATIBILITY.md            # Matriz de compatibilidad hosts y guests
Ôöé   ÔööÔöÇÔöÇ TECHNICAL_LIMITATIONS.md    # Restricciones PE, WHPX y persistencia
Ôö£ÔöÇÔöÇ crates/
Ôöé   Ôö£ÔöÇÔöÇ capsule-core/               # Primitivas, direcciones GPA/HVA e introspecci├│n host
Ôöé   Ôö£ÔöÇÔöÇ capsule-memory/             # Gesti├│n de memoria f├¡sica guest con VirtualAlloc
Ôöé   Ôö£ÔöÇÔöÇ capsule-cpu/                # Registros arquitecturales x86-64 y vCPU elemental
Ôöé   Ôö£ÔöÇÔöÇ capsule-devices/            # Dispositivos de plataforma (UART, PIC/PIT/RTC, PCI, IDE, virtio-net)
Ôöé   Ôö£ÔöÇÔöÇ capsule-network/            # Pila NAT en modo usuario para red guest
Ôöé   Ôö£ÔöÇÔöÇ capsule-storage/            # Discos RAW/VHD y overlay COW
Ôöé   Ôö£ÔöÇÔöÇ capsule-vmm/                # Orquestador de la VM, ciclo de vida y dispositivos
Ôöé   Ôö£ÔöÇÔöÇ capsule-whpx/               # Backend de virtualizacion por hardware WHPX (WinHvPlatform)
Ôöé   ÔööÔöÇÔöÇ capsule-gui/                # Aplicaci├│n ejecutable (capsule.exe) y ventana Win32
```

---

## Estado Actual de Desarrollo

El desarrollo se realiza de forma estrictamente incremental por hitos comprobados y probados:

* [x] **Hito M0 ÔÇö Host Diagnostics & Ventana Win32**:
  - Detecci├│n precisa de Windows x64 (Edici├│n y Build).
  - Detecci├│n de CPU (Nombre de modelo, fabricante, n├║cleos e instrucciones VT-x/SVM/Hypervisor).
  - Detecci├│n de memoria RAM f├¡sica total, disponible y porcentaje de carga.
  - Detecci├│n din├ímica en caliente de WHPX (`WinHvPlatform.dll`).
  - Ventana nativa Win32 optimizada (ClearType, interfaz oscura, cero frameworks pesados).
* [x] **Hito M1 ÔÇö VM M├¡nima & Ejecuci├│n Guest**:
  - Asignaci├│n segura de memoria f├¡sica guest (`GuestMemory`).
  - Carga de c├│digo m├íquina x86-64 en GPA.
  - Ejecuci├│n de vCPU paso a paso y detecci├│n de salidas VM (`HLT`, `I/O Port Out`).
  - Comando CLI interactivo: `capsule.exe test-guest`.
* [x] **Hito M2 ÔÇö Expansi├│n de CPU Emulada (x86_64)**:
  - Decodificaci├│n completa de instrucciones: aritm├®tica, l├│gica, ModR/M, SIB.
  - Prefijos REX para operaciones en modo de 64 bits completo y registros `R8`-`R15`.
  - Banderas `RFLAGS` (`ZF`, `CF`, `SF`, `OF`, `PF`), saltos condicionales `Jcc` y pila (`CALL`/`RET`).
* [x] **Hito M3 - Arranque Directo del Kernel Linux (bzImage direct boot)**:
  - Parser del encabezado `SetupHeader` con firma `HdrS`.
  - Construcci├│n de la *Zero-Page* (`boot_params`) y mapa de memoria f├¡sica E820.
  - Creaci├│n de tablas de p├íginas de 4 niveles (PML4) con *identity mapping* de 1 GB en p├íginas de 2MB.
  - Carga de l├¡nea de comandos del kernel e initramfs y ejecuci├│n guest v├¡a protocolo de arranque directo.
  - Comando CLI de validaci├│n: `capsule.exe test-boot`.
* [x] **Hito M4 - Linux Console & Shell (base UART / earlyprintk)**:
  - Emulaci├│n UART 16550 (COM1, 0x3F8) integrada como dispositivo virtual de la VM.
  - Soporte de E/S por puerto para salida de consola guest (`console=ttyS0` / earlyprintk).
  - Comando CLI de validaci├│n: `capsule.exe test-serial`.
* [x] **Hito M5 - Virtual Disk (RAW/VHD)**:
  - Abstracci├│n `BlockDevice` y backends `MemoryDisk`, `RawDiskImage`, `VhdFixedDisk` (fixed).
  - Controlador ATA/IDE PIO primario (canal maestro, LBA28): `IDENTIFY`, `READ/WRITE SECTORS` (y `READ/WRITE MULTIPLE`), `SET MULTIPLE MODE` (compat.), `FLUSH CACHE`.
  - Modelo de IRQ simplificado (condici├│n pendiente, `nIEN`, status vs alt-status) y software reset (`SRST`), con manejo de unidad ausente/slave y errores b├ísicos (`IDNF`/`ABRT`).
  - Integraci├│n VM: `attach_primary_master_disk`, E/S PIO 8/16/32-bit y arranque m├¡nimo desde disco (`boot_from_primary_master_disk`: carga LBA0 en `0x7C00`, `RIP=0x7C00`).
  - Validaci├│n: transferencias PIO (incl. multi-sector y `sector_count=0` => 256 sectores), guest READ/WRITE/FLUSH con persistencia en RAW/VHD fixed, y boot sector con carga multi-sector v├¡a IDE.
  - Comandos CLI: `capsule.exe test-storage`, `capsule.exe test-disk`, `capsule.exe test-boot-disk [path]` (self-test o arranque experimental de imagen RAW/VHD fixed 512-byte).
  - Alcance diferido: inyecci├│n de interrupciones en la CPU software (IDT/CLI/STI/IRET) para atender IRQs del PIC/PIT/IDE; mayor compatibilidad ATA/drivers y arranque real desde disco/firmware (M12); y formatos din├ímicos o `virtio-blk`.
* [x] **Hito M6 - Copy-on-Write Persistence**:
  - Overlay COW (`CowOverlayDisk`) sobre cualquier `BlockDevice` base (p. ej. RAW, VHD fixed), con bitmap por sector y persistencia en archivo de overlay.
  - Lectura transparente: sectores no modificados se leen de la base; sectores escritos se resuelven desde el overlay. Las escrituras nunca modifican la imagen base.
  - Validaci├│n de geometr├¡a base/overlay, reapertura con persistencia de cambios y operaci├│n a trav├®s del stack ATA/IDE+VM (guest READ/WRITE/FLUSH) sin alterar la base.
  - Comando CLI de validaci├│n: `capsule.exe test-cow`.
* [x] **Hito M7 - Virtual Platform Devices (PIC/PIT/RTC/PCI)**:
  - PIC 8259 dual (maestro/esclavo) con ICW/OCW, IMR/ISR/IRR, cascada y EOI; IRQ0 (PIT) e IRQ14 (IDE primario, nivel seg├║n `irq_pending && !nIEN`) reflejadas en el PIC.
  - PIT 8254 (canales 0-2, modos b├ísicos) con avance por tick de vCPU y pulsos IRQ0 (canal 0).
  - RTC/CMOS MC146818 m├¡nimo (puertos `0x70/0x71`): hora/fecha b├ísica y RAM CMOS preservada.
  - Bus PCI extensible (mecanismo #1, `0xCF8/0xCFC`) con enrutamiento de configuraci├│n/E/S y sondeo de dispositivos con acceso a memoria guest.
  - Integraci├│n en VM: dispositivos de plataforma registrados por defecto y refresco de timers/IRQs en el bucle de ejecuci├│n.
  - Comando CLI de validaci├│n: `capsule.exe test-platform`.
  - Nota: la inyecci├│n de interrupciones en la CPU software (IDT/CLI/STI/IRET) no est├í implementada a├║n; los guests deben usar sondeo (polling).
* [x] **Hito M8 - Networking (User-mode NAT / virtio-net)**:
  - Crate `capsule-network` con `UserModeNat`: ARP para gateway/DNS virtual, DHCP b├ísico (OFFER/ACK), ICMP echo (gateway/DNS virtual) y reenv├¡o UDP saliente mediante sockets del host (flujo UDP pendiente limitado).
  - Dispositivo `VirtioNetPci` transitional PCI legacy (I/O BAR0, split virtqueues RX/TX en memoria guest) en BDF `00:03.0`, operado por sondeo desde la VM (sin MMIO ni drivers host).
  - IRQ INTx PCI agregada se├▒alada en nivel al PIC como IRQ11; procesamiento de virtqueues/NAT realizado en el bucle de ejecuci├│n.
  - Comando CLI de validaci├│n: `capsule.exe test-network`.
  - Alcance diferido: TCP, m├║ltiples flujos/conexiones concurrentes, DNS real completo, features/offloads avanzados, inyecci├│n de IRQ en CPU (IDT/CLI/STI/IRET) y transporte moderno/MMIO.

* [x] **Hito M9 - Framebuffer & Window Blit (VGA/Bochs VBE)**:
  - Dispositivo `BochsVbe` (puertos DISPI `0x01CE/0x01CF`) con programaci├│n de modo/resoluci├│n/bpp y habilitaci├│n de LFB.
  - Framebuffer lineal mapeado en RAM guest en `0x0040_0000` (modelo sin MMIO, compatible con el backend software actual) y acceso desde `GuestMemory`.
  - Integraci├│n VMM/GUI: `step_once/run_steps` y copia de framebuffer a buffer BGRA32 para blit Win32 (`StretchDIBits`), renderizado en tiempo real en la ventana de `capsule.exe`.
  - Comando CLI de validaci├│n: `capsule.exe test-framebuffer`.
  - Limitaciones: LFB en RAM guest (no BAR MMIO dedicado), sin inyecci├│n de IRQ en CPU (IDT/CLI/STI/IRET), backend de render por sondeo/timer.


* [x] **Hito M10 - GUI Input & PS/2 (i8042)**:
  - Controlador PS/2 i8042 (`0x60/0x64`) en `capsule-devices` con teclado (scancodes Set 1) y rat├│n PS/2 (modo stream, paquetes de 3 bytes), integrado en la VM y expuesto mediante APIs de inyecci├│n desde `capsule-vmm`.
  - IRQ1 (teclado) e IRQ12 (rat├│n) reflejadas al PIC 8259 por sondeo en `tick_platform` seg├║n datos pendientes/command byte (sin inyecci├│n de interrupciones en la CPU software).
  - `capsule-gui` captura eventos Win32 (`WM_KEYDOWN/UP`, `WM_SYSKEY*`, `WM_MOUSEMOVE`, `WM_*BUTTON*`, foco) y los traduce a scancodes/paquetes PS/2; captura/liberaci├│n de rat├│n en ventana (clic para capturar, Ctrl+Esc para liberar).
  - Comando CLI de validaci├│n: `capsule.exe test-input`.
  - Limitaciones: sin inyecci├│n de IRQ en CPU (IDT/CLI/STI/IRET/APIC), mapeo de teclado parcial (Set 1, sin layout completo/IME), sin rueda/botones extendidos ni USB/HID, captura de rat├│n b├ísica.

* [x] **Hito M11 - WHPX Hardware Virtualization**:
  - Crate `capsule-whpx` con carga dinamica de `WinHvPlatform.dll` y wrapper minimo de WHPX: particion, vCPU, mapeo de `GuestMemory` como GPA, ejecucion `WHvRunVirtualProcessor` y sincronizacion basica de registros.
  - `capsule-vmm` soporta seleccion de backend (`VmBackendKind::{Software,Whpx}`) con API unificada de registros; `VirtualMachine::new()` intenta WHPX cuando se solicita y hace fallback a software si no esta disponible, y `new_whpx()` fuerza WHPX para validacion explicita.
  - VM-exits de E/S por puerto (IN/OUT) se traducen al despacho de dispositivos PIO/tick de plataforma existente, preservando el modelo actual de emulacion de dispositivos.
  - `capsule-gui` intenta WHPX por defecto en la GUI/demo (con fallback y reporte de estado) y agrega `capsule.exe test-whpx` (guest minimo HLT + IO a COM1 en WHPX, o SKIP si el host no lo soporta).
  - Limitaciones: sin MMIO, sin inyeccion/entrega arquitectural de IRQ (IDT/CLI/STI/IRET/APIC), contexto inicial minimo (sin long mode/firmware/kernel real), y fidelidad/temporizacion dependiente del modelo de tick/sondeo actual.
---

## Compilaci├│n y Uso

### Requisitos
- Windows 10 o Windows 11 (x64).
- Rust 1.80+ (`rustup default stable-x86_64-pc-windows-msvc`).

### Compilar el proyecto
```powershell
# Compilaci├│n en modo release
cargo build --release
```
El ejecutable resultante (`target\release\capsule.exe`) tiene un tama├▒o ligero de aproximadamente **200 KB**.

### Ejecutar la prueba de VM M├¡nima (Hito M1)
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
        Verificaci├│n de Ejecuci├│n Guest M1        
--------------------------------------------------
  Resultado en EAX  : 0x00000053 (83 decimal)
  RIP final         : 0x000000000000000B
  Estado de la VM   : Halted
--------------------------------------------------
```

### Ejecutar la aplicaci├│n gr├ífica (Hito M0)
```powershell
.\target\release\capsule.exe
```
Abre la ventana nativa de diagn├│stico con la informaci├│n del host y estado del runtime.

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
# Self-test sint├®tico (LBA0 -> 0x7C00, carga multi-sector LBA1-2 via ATA PIO)
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
# Valida PIC 8259, PIT 8254, RTC/CMOS y bus PCI (config 0xCF8/0xCFC), e integraci├│n b├ísica en VM
.\target\release\capsule.exe test-platform
```

### Verificar red en modo usuario (NAT + virtio-net PCI, Hito M8)
```powershell
# Valida ARP/ICMP en NAT de usuario y enumeraci├│n/configuraci├│n b├ísica de virtio-net (00:03.0, BAR0 I/O, MAC)
.\target\release\capsule.exe test-network
```


### Verificar framebuffer VGA/Bochs VBE y blit Win32 (Hito M9)
```powershell
# Programa VBE por puertos I/O, escribe en LFB guest (0x0040_0000) y valida copia BGRA32 para render
.\target\release\capsule.exe test-framebuffer
```

### Verificar entrada PS/2 i8042 (teclado/rat├│n, Hito M10)
```powershell
# Valida scancodes por 0x64/0x60, secuencia D4/F4+ACK+paquete de rat├│n e IRQ1/IRQ12 reflejadas al PIC (vectores 0x21/0x2C) por sondeo
.\target\release\capsule.exe test-input
```

### Verificar backend WHPX (aceleracion por hardware, Hito M11)
```powershell
# Ejecuta un guest minimo (HLT + IO a COM1) en WHPX si el host lo soporta; si no, reporta SKIP y CapsuleOS usa backend software
.\target\release\capsule.exe test-whpx
```

### Ejecutar la bater├¡a de pruebas automatizadas
```powershell
cargo test --workspace
```

---

## Documentaci├│n T├®cnica

Para conocer m├ís detalles sobre la arquitectura interna y los desaf├¡os t├®cnicos:
- [Arquitectura de Subsistemas (docs/ARCHITECTURE.md)](docs/ARCHITECTURE.md)
- [Hoja de Ruta Completa (docs/ROADMAP.md)](docs/ROADMAP.md)
- [Matriz de Compatibilidad (docs/COMPATIBILITY.md)](docs/COMPATIBILITY.md)
- [Limitaciones T├®cnicas y Retos de Dise├▒o (docs/TECHNICAL_LIMITATIONS.md)](docs/TECHNICAL_LIMITATIONS.md)

---

## ­ƒôä Licencia

Este proyecto est├í bajo la licencia MIT o Apache-2.0.
