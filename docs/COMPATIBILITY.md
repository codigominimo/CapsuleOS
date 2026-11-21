# Matriz de Compatibilidad de CapsuleOS

Este documento describe la compatibilidad planeada y verificada de CapsuleOS tanto a nivel de sistema host como de sistemas operativos invitados (guests).

> **Regla de integridad**: Un sistema operativo o funcionalidad solo se marcará como **SOPORTADO (VERIFICADO)** tras haberse completado una prueba real de ejecución automatizada o documentada. En fases previas se marcará como **PLANEADO** o **EN DESARROLLO**.

---

## 1. Requisitos del Host (Windows)

| Componente | Requisito Mínimo | Requisito Recomendado |
|---|---|---|
| **Sistema Operativo** | Windows 10 x64 (Build 1809+) / Windows 11 x64 | Windows 11 x64 (Build 22H2+) |
| **Arquitectura Host** | x86_64 (Intel 64 o AMD64) | x86_64 con VT-x / AMD-V y SLAT |
| **Virtualización Hardware** | Opcional (Modo Emulador) | Recomendado para WHPX: "Windows Hypervisor Platform" habilitado |
| **Memoria RAM** | 4 GB | 16 GB o superior |
| **Tarjeta Gráfica** | GPU compatible con DirectX 11 o GDI básico | GPU con aceleración D3D11/D3D12 |

---

## 2. Matriz de Sistemas Invitados (Guest OS)

| Sistema Operativo | Arquitectura | Modo Arranque | Backend Emulador | Backend WHPX | Consola (TTY) | Gráficos (GUI) | Entrada (PS/2) | Persistencia | Estado |
|---|---|---|---|---|---|---|---|---|---|
| **Minimal Test Guest (synthetic)** | x86-64 | Direct (M1/M11 tests) | Verificado (backend software) | Verificado parcial (tests `test-whpx`: HLT + IO a COM1, según host) | Verificado (UART 16550/COM1 por sondeo) | Verificado (VBE LFB 320x200x32, `test-framebuffer`, render Win32 por sondeo) | Verificado (i8042 teclado/ratón, IRQ1/IRQ12 por sondeo, `test-input`; GUI Win32→PS/2) | N/A | En desarrollo (tests internos, WHPX mínimo) |
| **Linux Minimal (BusyBox / initramfs)** | x86-64 | Direct Boot (bzImage) | Planeado | Planeado | Planeado (base UART/earlyprintk) | N/A | Planeado | Planeado | Planeado |
| **Alpine Linux** | x86-64 | Direct / BIOS | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Ubuntu Server** | x86-64 | UEFI / Direct | Planeado | Planeado | Planeado | N/A | Planeado | Planeado | Planeado |
| **Ubuntu Desktop** | x86-64 | UEFI | No (lento) | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Debian GNU/Linux** | x86-64 | BIOS / UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Fedora Linux** | x86-64 | UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Windows PE** | x86-64 | UEFI | No | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Windows 10 / 11** | x86-64 | UEFI + TPM2 | No | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **FreeBSD** | x86-64 | BIOS / UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |

---

## 3. Formatos de Almacenamiento y Medios Soportados

| Formato | Lectura | Escritura Directa | Capa Copy-on-Write (Overlay) | Estado |
|---|---|---|---|---|
| **Raw Sector Image (.img, .raw)** | Verificado (tests M5) | Verificado (tests M5) | Verificado (overlay COW, M6) | Soportado (RAW sector 512, scope actual) |
| **VHD Fixed (512-byte)** | Verificado (tests M5) | Verificado (tests M5) | Verificado (overlay COW, M6) | Soportado (fixed, sector 512) |
| **ISO 9660 (.iso)** | Planeado (M12.2, ATAPI/El Torito no implementado) | No (Solo lectura) | Planeado (Overlay COW) | Planeado |
| **VHD Dynamic** | Planeado | Planeado | Planeado | Planeado |
| **VHDX** | Planeado | Planeado | Planeado | Planeado |
| **QCOW2** | Planeado | Planeado | Planeado | Planeado |

---

## 4. Plataforma Virtual, Red, Gráficos y Entrada (M7-M10)

| Componente | Capacidad verificada (tests CLI/unitarios, backend software) | Limitaciones actuales |
|---|---|---|
| **PIC 8259 / PIT 8254 / RTC-CMOS** | Inicialización, IMR/IRQ0 PIT, hora/fecha básica y tick simplificado (`test-platform`) | Sin inyección de IRQ en CPU (IDT/CLI/STI/IRET/APIC); modelo temporal simplificado |
| **PCI (mecanismo #1)** | Enumeración/config I/O `0xCF8/0xCFC`, BAR I/O y dispositivo virtio-net en `00:03.0` | Sin MMIO; INTx agregada cableada a IRQ11 por sondeo |
| **Virtio-net transitional (legacy I/O)** | Virtqueues RX/TX en memoria guest y procesamiento por polling desde la VM (`test-network`) | Sin MMIO/transporte moderno, sin features/offloads avanzados |
| **User-mode NAT** | ARP, DHCP básico, ICMP echo (gateway/DNS virtual), UDP saliente con respuesta (flujo limitado) (`test-network`) | Sin TCP, sin multi-flow/conexiones concurrentes completas, sin DNS real completo, sin drivers host |
| **VGA/Bochs VBE + LFB (M9)** | Programación DISPI por I/O (`0x01CE/0x01CF`), modo 320x200x32 con LFB en RAM guest (`0x0040_0000`), escritura guest y blit host BGRA32 en ventana Win32 (`test-framebuffer`) | LFB en RAM guest (sin MMIO/BAR dedicado), sin VBlank/IRQ, sin aceleración 2D/3D, compatibilidad VBE parcial |
| **PS/2 i8042 teclado/ratón (M10)** | Puertos `0x60/0x64`, scancodes Set 1 y paquetes de ratón (3 bytes) leíbles por guest, IRQ1/IRQ12 reflejadas al PIC (vectores 0x21/0x2C) por sondeo (`test-input`); GUI Win32 traduce eventos a entrada PS/2 | Sin inyección de IRQ en CPU, mapeo de teclado parcial (sin layout completo/IME), sin rueda/botones extendidos ni USB/HID, captura de ratón básica |

## 5. Backends de Ejecución (M11)

| Backend | Estado | Requisitos host | Alcance verificado | Limitaciones actuales |
|---|---|---|---|---|
| **Software (x86-64 interpreter, `capsule-cpu`/`capsule-vmm`)** | Verificado (tests internos) | Windows x64 | Ejecución de guests de prueba y rutas CLI existentes (M1-M10), dispositivos PIO por sondeo | Sin inyección arquitectural de IRQ (IDT/CLI/STI/IRET/APIC), sin MMIO, emulación parcial de ISA/modos |
| **WHPX (Windows Hypervisor Platform, `capsule-whpx`)** | Verificado parcial (según host) | Windows x64 con "Windows Hypervisor Platform" habilitada y VT-x/AMD-V activo; si no, `test-whpx` reporta SKIP y `VirtualMachine::new()` hace fallback a software | Partición/vCPU WHPX, mapeo GPA de memoria guest, VM-exits de IO port (IN/OUT) hacia dispositivos emulados, guest mínimo HLT + OUT a COM1 (`test-whpx`); GUI intenta WHPX por defecto con fallback | Sin MMIO, sin inyección/entrega arquitectural de IRQ, contexto inicial mínimo (no long mode/firmware/kernel real), decodificación de IO exit/longitud dependiente de heurísticas/fallback, 1 vCPU |
