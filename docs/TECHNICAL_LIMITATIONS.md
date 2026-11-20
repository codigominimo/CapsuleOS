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

### 2.1. Requisitos y Permisos de WHPX
* **Requisito**: Windows Hypervisor Platform requiere que el procesador cuente con soporte de virtualización por hardware (Intel VT-x o AMD-V) habilitado en el firmware UEFI/BIOS del equipo host.
* **Función de Windows**: La característica opcional `HypervisorPlatform` de Windows debe estar activa. Si Hyper-V está deshabilitado a nivel de sistema operativo, WHPX devolverá un error al inicializar particiones.
* **Fallback a Emulador**: Para garantizar que CapsuleOS funcione en cualquier ordenador (incluyendo máquinas virtuales anidadas que no soporten virtualización anidada), CapsuleOS integrará un backend emulador de CPU x86-64 por software.

### 2.2. Rendimiento de la Emulación por Software
* **Limitación**: Un intérprete de instrucciones x86_64 en modo software es órdenes de magnitud más lento que la virtualización por hardware nativa.
* **Objetivo de uso**: La emulación por software está destinada a sistemas minimalistas (kernels embebidos, pruebas, NanoOS o Linux mínimo de recuperación) y no para sistemas de escritorio pesados con entornos gráficos complejos hasta no disponer de un motor de traducción binaria dinámica (JIT).

### 2.3. Plataforma/IRQ en la CPU software (estado M7-M8)
* **Limitación**: El backend de CPU software actual no implementa entrega de interrupciones arquitecturales (IDT/CLI/STI/IRET/APIC). Aunque el PIC/PIT/RTC/PCI/IDE reflejan IRQs y condiciones pendientes (p. ej. PIT IRQ0, IDE IRQ14, INTx PCI/IRQ11), la VM procesa dispositivos por sondeo (polling) en cada tick/iteración.
* **Impacto**: Guests que dependan estrictamente de interrupciones temporizadas o de notificación asíncrona por IRQ pueden no funcionar correctamente hasta que se implemente inyección/entrega de interrupciones en la vCPU.

---

## 3. Dispositivos y Controladores

### 3.1. Red sin Privilegios Administrativos (M8: implementación inicial)
* **Problema**: La creación de interfaces de red virtuales en Windows (TAP/Wintun) suele requerir privilegios de Administrador.
* **Estado M8**: `capsule-network::UserModeNat` implementa una pila/NAT en modo usuario básica (ARP, DHCP básico, ICMP echo a IPs virtuales, reenvío UDP saliente mediante sockets del host con un flujo pendiente) expuesta al guest por `VirtioNetPci` (PCI legacy I/O BAR0, virtqueues RX/TX en memoria guest) operada por sondeo desde la VM.
* **Limitaciones actuales**: sin TCP y sin múltiples flujos/conexiones concurrentes completas; DNS no es resolución real completa (uso de IPs virtuales/reenvío limitado); sin MMIO/transporte moderno y sin inyección de IRQ en CPU (IDT/CLI/STI/IRET), por lo que el dispositivo depende de polling; INTx PCI se agrega como IRQ11 en PIC a nivel de plataforma.

### 3.2. Aceleración Gráfica
* **Limitación**: El acceso directo a GPU (GPU Passthrough) no está disponible en hipervisores de tipo 2 de nivel de usuario en Windows.
* **Solución**: Proporcionar un adaptador de framebuffer de alto rendimiento (Bochs VBE / VESA) con transferencia directa de búfer mediante GDI DIBSection / Direct3D 11 swap chain para alcanzar 60 FPS estables en la interfaz gráfica del SO invitado.
