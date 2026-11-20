# Matriz de Compatibilidad de CapsuleOS

Este documento describe la compatibilidad planeada y verificada de CapsuleOS tanto a nivel de sistema host como de sistemas operativos invitados (guests).

> **Regla de integridad**: Un sistema operativo o funcionalidad solo se marcará como **SOPORTADO (VERIFICADO)** tras haberse completado una prueba real de ejecución automatizada o documentada. En fases previas se marcará como **PLANEADO** o **EN DESARROLLO**.

---

## 1. Requisitos del Host (Windows)

| Componente | Requisito Mínimo | Requisito Recomendado |
|---|---|---|
| **Sistema Operativo** | Windows 10 x64 (Build 1809+) / Windows 11 x64 | Windows 11 x64 (Build 22H2+) |
| **Arquitectura Host** | x86_64 (Intel 64 o AMD64) | x86_64 con VT-x / AMD-V y SLAT |
| **Virtualización Hardware** | Opcional (Modo Emulador) | Obligatorio para WHPX: "Windows Hypervisor Platform" habilitado |
| **Memoria RAM** | 4 GB | 16 GB o superior |
| **Tarjeta Gráfica** | GPU compatible con DirectX 11 o GDI básico | GPU con aceleración D3D11/D3D12 |

---

## 2. Matriz de Sistemas Invitados (Guest OS)

| Sistema Operativo | Arquitectura | Modo Arranque | Backend Emulador | Backend WHPX | Consola (TTY) | Gráficos (GUI) | Persistencia | Estado |
|---|---|---|---|---|---|---|---|---|
| **Minimal Test Guest** | x86-64 | Direct (M1) | Planeado | Planeado | Planeado | N/A | N/A | En desarrollo |
| **Linux Minimal (BusyBox)** | x86-64 | Direct Boot | Planeado | Planeado | Planeado | N/A | Planeado | Planeado |
| **Alpine Linux** | x86-64 | Direct / BIOS | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Ubuntu Server** | x86-64 | UEFI / Direct | Planeado | Planeado | Planeado | N/A | Planeado | Planeado |
| **Ubuntu Desktop** | x86-64 | UEFI | No (lento) | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Debian GNU/Linux** | x86-64 | BIOS / UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Fedora Linux** | x86-64 | UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Windows PE** | x86-64 | UEFI | No | Planeado | Planeado | Planeado | Planeado | Planeado |
| **Windows 10 / 11** | x86-64 | UEFI + TPM2 | No | Planeado | Planeado | Planeado | Planeado | Planeado |
| **FreeBSD** | x86-64 | BIOS / UEFI | Planeado | Planeado | Planeado | Planeado | Planeado | Planeado |

---

## 3. Formatos de Almacenamiento y Medios Soportados

| Formato | Lectura | Escritura Directa | Capa Copy-on-Write (Overlay) | Estado |
|---|---|---|---|---|
| **Raw Sector Image (.img, .raw)** | Planeado | Planeado | Planeado | Planeado |
| **ISO 9660 (.iso)** | Planeado | No (Solo lectura) | Planeado (Overlay COW) | Planeado |
| **VHD (Fixed / Dynamic)** | Planeado | Planeado | Planeado | Planeado |
| **VHDX** | Planeado | Planeado | Planeado | Planeado |
| **QCOW2** | Planeado | Planeado | Planeado | Planeado |
