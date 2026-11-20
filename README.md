# CapsuleOS 🚀

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
│   ├── capsule-vmm/                # Orquestador de la VM, ciclo de vida y dispositivos
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
  - Alcance diferido: INTRQ real vía PIC/APIC (M7), mayor compatibilidad ATA/drivers y arranque real desde disco/firmware (M12), y formatos dinámicos o `virtio-blk`.
* [x] **Hito M6 - Copy-on-Write Persistence**:
  - Overlay COW (`CowOverlayDisk`) sobre cualquier `BlockDevice` base (p. ej. RAW, VHD fixed), con bitmap por sector y persistencia en archivo de overlay.
  - Lectura transparente: sectores no modificados se leen de la base; sectores escritos se resuelven desde el overlay. Las escrituras nunca modifican la imagen base.
  - Validación de geometría base/overlay, reapertura con persistencia de cambios y operación a través del stack ATA/IDE+VM (guest READ/WRITE/FLUSH) sin alterar la base.
  - Comando CLI de validación: `capsule.exe test-cow`.

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
