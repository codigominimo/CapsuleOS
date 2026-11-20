//! Estructuras de datos para el protocolo de arranque x86 de Linux (Zero-Page / boot_params).

use capsule_core::types::GuestAddress;

pub const HDRS_MAGIC: u32 = 0x53726448; // "HdrS"
pub const E820_RAM: u32 = 1;
pub const E820_RESERVED: u32 = 2;
pub const E820_ACPI: u32 = 3;
pub const E820_NVS: u32 = 4;

/// Entrada de mapa de memoria físico E820 de la BIOS/UEFI para Linux.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct E820Entry {
    pub addr: u64,
    pub size: u64,
    pub entry_type: u32,
}

/// Encabezado de configuración del kernel Linux (offset 0x1F1 del bzImage).
#[derive(Debug, Clone)]
pub struct SetupHeader {
    pub setup_sects: u8,
    pub root_flags: u16,
    pub syssize: u32,
    pub ram_size: u16,
    pub vid_mode: u16,
    pub root_dev: u16,
    pub boot_flag: u16,
    pub jump: u16,
    pub header: u32, // Debe ser "HdrS" (0x53726448)
    pub version: u16,
    pub realmode_swtch: u32,
    pub start_sys_seg: u16,
    pub kernel_version: u16,
    pub type_of_loader: u8,
    pub loadflags: u8,
    pub setup_move_size: u16,
    pub code32_start: u32,
    pub ramdisk_image: u32,
    pub ramdisk_size: u32,
    pub bootsect_kludge: u32,
    pub heap_end_ptr: u16,
    pub ext_loader_ver: u8,
    pub ext_loader_type: u8,
    pub cmd_line_ptr: u32,
    pub initrd_addr_max: u32,
    pub kernel_alignment: u32,
    pub relocatable_kernel: u8,
    pub min_alignment: u8,
    pub xloadflags: u16,
    pub cmdline_size: u32,
    pub hardware_subarch: u32,
    pub hardware_subarch_data: u64,
    pub payload_offset: u32,
    pub payload_length: u32,
    pub setup_data: u64,
    pub pref_address: u64,
    pub init_size: u32,
    pub handover_offset: u32,
}

impl SetupHeader {
    /// Parsea el SetupHeader a partir de un slice del binario bzImage.
    pub fn parse(kernel_bytes: &[u8]) -> Result<Self, String> {
        if kernel_bytes.len() < 0x270 {
            return Err("Imagen de kernel demasiado pequeña para contener el SetupHeader".to_string());
        }

        let magic = u32::from_le_bytes(kernel_bytes[0x202..0x206].try_into().unwrap());
        if magic != HDRS_MAGIC {
            return Err(format!(
                "Firma de kernel no válida: esperado 'HdrS' (0x53726448), encontrado 0x{:08X}",
                magic
            ));
        }

        let setup_sects_raw = kernel_bytes[0x1F1];
        let setup_sects = if setup_sects_raw == 0 { 4 } else { setup_sects_raw };

        let header = Self {
            setup_sects,
            root_flags: u16::from_le_bytes(kernel_bytes[0x1F2..0x1F4].try_into().unwrap()),
            syssize: u32::from_le_bytes(kernel_bytes[0x1F4..0x1F8].try_into().unwrap()),
            ram_size: u16::from_le_bytes(kernel_bytes[0x1F8..0x1FA].try_into().unwrap()),
            vid_mode: u16::from_le_bytes(kernel_bytes[0x1FA..0x1FC].try_into().unwrap()),
            root_dev: u16::from_le_bytes(kernel_bytes[0x1FC..0x1FE].try_into().unwrap()),
            boot_flag: u16::from_le_bytes(kernel_bytes[0x1FE..0x200].try_into().unwrap()),
            jump: u16::from_le_bytes(kernel_bytes[0x200..0x202].try_into().unwrap()),
            header: magic,
            version: u16::from_le_bytes(kernel_bytes[0x206..0x208].try_into().unwrap()),
            realmode_swtch: u32::from_le_bytes(kernel_bytes[0x208..0x20C].try_into().unwrap()),
            start_sys_seg: u16::from_le_bytes(kernel_bytes[0x20C..0x20E].try_into().unwrap()),
            kernel_version: u16::from_le_bytes(kernel_bytes[0x20E..0x210].try_into().unwrap()),
            type_of_loader: kernel_bytes[0x210],
            loadflags: kernel_bytes[0x211],
            setup_move_size: u16::from_le_bytes(kernel_bytes[0x212..0x214].try_into().unwrap()),
            code32_start: u32::from_le_bytes(kernel_bytes[0x214..0x218].try_into().unwrap()),
            ramdisk_image: u32::from_le_bytes(kernel_bytes[0x218..0x21C].try_into().unwrap()),
            ramdisk_size: u32::from_le_bytes(kernel_bytes[0x21C..0x220].try_into().unwrap()),
            bootsect_kludge: u32::from_le_bytes(kernel_bytes[0x220..0x224].try_into().unwrap()),
            heap_end_ptr: u16::from_le_bytes(kernel_bytes[0x224..0x226].try_into().unwrap()),
            ext_loader_ver: kernel_bytes[0x226],
            ext_loader_type: kernel_bytes[0x227],
            cmd_line_ptr: u32::from_le_bytes(kernel_bytes[0x228..0x22C].try_into().unwrap()),
            initrd_addr_max: u32::from_le_bytes(kernel_bytes[0x22C..0x230].try_into().unwrap()),
            kernel_alignment: u32::from_le_bytes(kernel_bytes[0x230..0x234].try_into().unwrap()),
            relocatable_kernel: kernel_bytes[0x234],
            min_alignment: kernel_bytes[0x235],
            xloadflags: u16::from_le_bytes(kernel_bytes[0x236..0x238].try_into().unwrap()),
            cmdline_size: u32::from_le_bytes(kernel_bytes[0x238..0x23C].try_into().unwrap()),
            hardware_subarch: u32::from_le_bytes(kernel_bytes[0x23C..0x240].try_into().unwrap()),
            hardware_subarch_data: u64::from_le_bytes(kernel_bytes[0x240..0x248].try_into().unwrap()),
            payload_offset: u32::from_le_bytes(kernel_bytes[0x248..0x24C].try_into().unwrap()),
            payload_length: u32::from_le_bytes(kernel_bytes[0x24C..0x250].try_into().unwrap()),
            setup_data: u64::from_le_bytes(kernel_bytes[0x250..0x258].try_into().unwrap()),
            pref_address: u64::from_le_bytes(kernel_bytes[0x258..0x260].try_into().unwrap()),
            init_size: u32::from_le_bytes(kernel_bytes[0x260..0x264].try_into().unwrap()),
            handover_offset: u32::from_le_bytes(kernel_bytes[0x264..0x268].try_into().unwrap()),
        };

        Ok(header)
    }

    /// Retorna el desplazamiento en bytes dentro del bzImage donde comienza el kernel ejecutable de 32/64 bits.
    pub fn kernel_code_offset(&self) -> usize {
        (self.setup_sects as usize + 1) * 512
    }
}

/// Parámetros de arranque del kernel (Zero-Page en GPA 0x7000 o 0x10000).
pub struct BootParams {
    pub setup_header: SetupHeader,
    pub e820_map: Vec<E820Entry>,
    pub cmdline_addr: GuestAddress,
    pub initramfs_addr: Option<GuestAddress>,
    pub initramfs_size: usize,
}

impl BootParams {
    pub fn new(setup_header: SetupHeader, cmdline_addr: GuestAddress) -> Self {
        Self {
            setup_header,
            e820_map: Vec::new(),
            cmdline_addr,
            initramfs_addr: None,
            initramfs_size: 0,
        }
    }

    /// Añade una región de memoria al mapa E820.
    pub fn add_e820_entry(&mut self, addr: u64, size: u64, entry_type: u32) {
        self.e820_map.push(E820Entry {
            addr,
            size,
            entry_type,
        });
    }

    /// Serializa los parámetros de arranque en un bloque de 4096 bytes listo para escribir en RAM guest.
    pub fn serialize(&self) -> [u8; 4096] {
        let mut page = [0u8; 4096];

        // 1. Copiar y configurar SetupHeader (offset 0x1F1 a 0x290)
        page[0x1F1] = self.setup_header.setup_sects;
        page[0x202..0x206].copy_from_slice(&HDRS_MAGIC.to_le_bytes());
        page[0x206..0x208].copy_from_slice(&self.setup_header.version.to_le_bytes());
        page[0x210] = 0xFF; // type_of_loader: Qemu/VMM loader
        page[0x211] = self.setup_header.loadflags | 0x80 | 0x01; // CAN_USE_HEAP | LOADED_HIGH
        page[0x214..0x218].copy_from_slice(&self.setup_header.code32_start.to_le_bytes());

        // Dirección de initrd/initramfs
        if let Some(initrd) = self.initramfs_addr {
            page[0x218..0x21C].copy_from_slice(&(initrd.raw() as u32).to_le_bytes());
            page[0x21C..0x220].copy_from_slice(&(self.initramfs_size as u32).to_le_bytes());
        }

        // Puntero a la línea de comandos
        page[0x228..0x22C].copy_from_slice(&(self.cmdline_addr.raw() as u32).to_le_bytes());

        // 2. Número de entradas E820 (offset 0x1E8, 1 byte)
        let num_entries = self.e820_map.len().min(128) as u8;
        page[0x1E8] = num_entries;

        // 3. Tabla E820 (offset 0x2D0, hasta 128 entradas de 20 bytes cada una)
        let mut offset = 0x2D0;
        for entry in self.e820_map.iter().take(128) {
            page[offset..offset + 8].copy_from_slice(&entry.addr.to_le_bytes());
            page[offset + 8..offset + 16].copy_from_slice(&entry.size.to_le_bytes());
            page[offset + 16..offset + 20].copy_from_slice(&entry.entry_type.to_le_bytes());
            offset += 20;
        }

        page
    }
}
