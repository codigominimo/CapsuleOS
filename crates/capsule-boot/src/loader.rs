//! Cargador de kernel Linux (bzImage / vmlinuz direct boot).

use crate::bootparams::{BootParams, SetupHeader, E820_RAM, E820_RESERVED};
use crate::pagetables::setup_identity_pagetables;
use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;

pub const BOOT_PARAMS_ADDR: GuestAddress = GuestAddress(0x7000);
pub const CMDLINE_ADDR: GuestAddress = GuestAddress(0x8000);
pub const KERNEL_LOAD_ADDR: GuestAddress = GuestAddress(0x100000); // 1 MB (bzImage default entry)

#[derive(Debug, Clone)]
pub struct LinuxBootInfo {
    pub entry_rip: u64,
    pub boot_params_addr: GuestAddress,
    pub pml4_addr: GuestAddress,
    pub kernel_size: usize,
    pub initramfs_size: usize,
    pub cmdline: String,
}

pub struct LinuxKernelLoader;

impl LinuxKernelLoader {
    /// Carga un binario de kernel Linux (bzImage), configura la línea de comandos,
    /// el initramfs y los parámetros de arranque (Zero Page) en la memoria física del guest.
    pub fn load(
        mem: &mut GuestMemory,
        kernel_bytes: &[u8],
        cmdline: &str,
        initramfs_bytes: Option<&[u8]>,
    ) -> Result<LinuxBootInfo, String> {
        // 1. Parsear y validar el SetupHeader
        let setup_header = SetupHeader::parse(kernel_bytes)?;

        // 2. Configurar tablas de páginas x86-64 con identity mapping (PML4 en 0x9000)
        let pml4_addr = setup_identity_pagetables(mem)?;

        // 3. Escribir la línea de comandos del kernel en 0x8000 (terminada en null)
        let mut cmdline_buf = cmdline.as_bytes().to_vec();
        cmdline_buf.push(0);
        mem.write_bytes(CMDLINE_ADDR, &cmdline_buf)
            .map_err(|e| format!("Error escribiendo cmdline en {}: {}", CMDLINE_ADDR, e))?;

        // 4. Cargar el cuerpo ejecutable del kernel (código de 32/64 bits) a partir de 1 MB
        let kernel_offset = setup_header.kernel_code_offset();
        if kernel_offset >= kernel_bytes.len() {
            return Err("El desplazamiento del código del kernel excede el tamaño del archivo".to_string());
        }
        let kernel_code = &kernel_bytes[kernel_offset..];
        mem.write_bytes(KERNEL_LOAD_ADDR, kernel_code)
            .map_err(|e| format!("Error copiando kernel a {}: {}", KERNEL_LOAD_ADDR, e))?;

        // 5. Cargar initramfs si está presente (ubicado en memoria alta, e.g. 64 MB o bajo el tope de memoria)
        let mut initramfs_addr_opt = None;
        let mut initramfs_size = 0;

        if let Some(initramfs) = initramfs_bytes {
            if !initramfs.is_empty() {
                // Colocar el initramfs alineado a 4KB dejando margen para el kernel
                let initramfs_start = GuestAddress::new(0x04000000); // 64 MB
                if mem.is_valid_range(initramfs_start, initramfs.len()) {
                    mem.write_bytes(initramfs_start, initramfs)
                        .map_err(|e| format!("Error cargando initramfs: {}", e))?;
                    initramfs_addr_opt = Some(initramfs_start);
                    initramfs_size = initramfs.len();
                } else {
                    return Err(format!(
                        "Memoria guest insuficiente para alojar initramfs de {} bytes en {}",
                        initramfs.len(),
                        initramfs_start
                    ));
                }
            }
        }

        // 6. Construir el mapa de memoria E820
        let mut boot_params = BootParams::new(setup_header.clone(), CMDLINE_ADDR);
        boot_params.initramfs_addr = initramfs_addr_opt;
        boot_params.initramfs_size = initramfs_size;

        let total_mem = mem.size() as u64;

        // Región baja: 0 a 640 KB
        boot_params.add_e820_entry(0x00000000, 0x0009FC00, E820_RAM);
        // Región reservada de BIOS/VGA: 640 KB a 1 MB
        boot_params.add_e820_entry(0x0009FC00, 0x00060400, E820_RESERVED);
        // Memoria extendida: 1 MB al tope de la memoria RAM guest
        if total_mem > 0x00100000 {
            boot_params.add_e820_entry(0x00100000, total_mem - 0x00100000, E820_RAM);
        }

        // 7. Serializar y escribir BootParams (Zero-Page) en GPA 0x7000
        let zero_page = boot_params.serialize();
        mem.write_bytes(BOOT_PARAMS_ADDR, &zero_page)
            .map_err(|e| format!("Error escribiendo boot_params en {}: {}", BOOT_PARAMS_ADDR, e))?;

        // 8. Determinar punto de entrada
        // Para bzImage moderno, code32_start suele ser 0x100000 (1 MB)
        let entry_rip = if setup_header.code32_start != 0 {
            setup_header.code32_start as u64
        } else {
            KERNEL_LOAD_ADDR.raw()
        };

        Ok(LinuxBootInfo {
            entry_rip,
            boot_params_addr: BOOT_PARAMS_ADDR,
            pml4_addr,
            kernel_size: kernel_code.len(),
            initramfs_size,
            cmdline: cmdline.to_string(),
        })
    }
}
