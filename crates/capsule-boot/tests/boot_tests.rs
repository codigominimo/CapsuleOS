use capsule_boot::{
    LinuxKernelLoader, BOOT_PARAMS_ADDR, CMDLINE_ADDR, HDRS_MAGIC, KERNEL_LOAD_ADDR, PML4_ADDRESS,
};
use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;

/// Genera una imagen bzImage sintética válida según la especificación del protocolo de arranque x86 de Linux.
fn create_synthetic_bzimage(kernel_payload: &[u8]) -> Vec<u8> {
    let setup_sects = 4u8;
    let header_size = (setup_sects as usize + 1) * 512; // 2560 bytes
    let total_size = header_size + kernel_payload.len();
    let mut image = vec![0u8; total_size];

    // Offset 0x1F1: setup_sects
    image[0x1F1] = setup_sects;
    // Offset 0x202: magic "HdrS" (0x53726448)
    image[0x202..0x206].copy_from_slice(&HDRS_MAGIC.to_le_bytes());
    // Offset 0x206: boot protocol version (0x020D = Linux 2.13)
    image[0x206..0x208].copy_from_slice(&0x020Du16.to_le_bytes());
    // Offset 0x211: loadflags (LOADED_HIGH = 0x01)
    image[0x211] = 0x01;
    // Offset 0x214: code32_start (0x100000 = 1 MB)
    image[0x214..0x218].copy_from_slice(&0x00100000u32.to_le_bytes());

    // Copiar el código del kernel tras los sectores de setup
    image[header_size..].copy_from_slice(kernel_payload);

    image
}

#[test]
fn test_linux_bzimage_parsing_and_zero_page() {
    let mut mem = GuestMemory::new(128 * 1024 * 1024).expect("Fallo reservando 128MB RAM"); // 128 MB

    // Código representativo de un punto de entrada de kernel:
    // mov eax, 0x1000
    // hlt
    let dummy_kernel_code = [0xB8, 0x00, 0x10, 0x00, 0x00, 0xF4];
    let bzimage = create_synthetic_bzimage(&dummy_kernel_code);

    let cmdline = "console=ttyS0 earlyprintk=serial,0x3F8,115200 root=/dev/vda";
    let initramfs = b"FAKE_INITRAMFS_ARCHIVE_DATA";

    let boot_info = LinuxKernelLoader::load(&mut mem, &bzimage, cmdline, Some(initramfs))
        .expect("Fallo al cargar kernel Linux");

    // 1. Verificar información de arranque
    assert_eq!(boot_info.entry_rip, 0x100000, "RIP debe apuntar a 1MB");
    assert_eq!(boot_info.boot_params_addr, BOOT_PARAMS_ADDR);
    assert_eq!(boot_info.pml4_addr, PML4_ADDRESS);
    assert_eq!(boot_info.kernel_size, dummy_kernel_code.len());
    assert_eq!(boot_info.initramfs_size, initramfs.len());

    // 2. Verificar que el código del kernel fue copiado a 1 MB
    let mut read_code = vec![0u8; dummy_kernel_code.len()];
    mem.read_bytes(KERNEL_LOAD_ADDR, &mut read_code).unwrap();
    assert_eq!(read_code, dummy_kernel_code, "El código del kernel debe coincidir en 1MB");

    // 3. Verificar la línea de comandos en 0x8000
    let mut read_cmdline = vec![0u8; cmdline.len()];
    mem.read_bytes(CMDLINE_ADDR, &mut read_cmdline).unwrap();
    assert_eq!(
        String::from_utf8(read_cmdline).unwrap(),
        cmdline,
        "La línea de comandos debe coincidir en 0x8000"
    );

    // 4. Verificar la Zero-Page en 0x7000
    let magic = mem.read_u32(GuestAddress::new(0x7000 + 0x202)).unwrap();
    assert_eq!(magic, HDRS_MAGIC, "La firma HdrS debe estar presente en la Zero Page");

    let num_e820 = mem.read_u8(GuestAddress::new(0x7000 + 0x1E8)).unwrap();
    assert!(num_e820 >= 2, "Debe haber al menos 2 entradas E820");

    let cmdline_ptr = mem.read_u32(GuestAddress::new(0x7000 + 0x228)).unwrap();
    assert_eq!(cmdline_ptr, 0x8000, "El puntero a la línea de comandos en la Zero Page debe ser 0x8000");

    println!("Linux Kernel Loader Test completado con éxito");
    println!("Boot Info: {:?}", boot_info);
}

#[test]
fn test_pagetables_identity_mapping() {
    let mut mem = GuestMemory::new(64 * 1024 * 1024).unwrap();
    let pml4 = capsule_boot::setup_identity_pagetables(&mut mem).unwrap();
    assert_eq!(pml4, PML4_ADDRESS);

    // Comprobar que la primera entrada PML4 apunta a PDPT
    let pml4_0 = mem.read_u64(PML4_ADDRESS).unwrap();
    assert_eq!(pml4_0 & !0xFFF, capsule_boot::PDPT_ADDRESS.raw());

    // Comprobar que la primera entrada PDPT apunta a PD
    let pdpt_0 = mem.read_u64(capsule_boot::PDPT_ADDRESS).unwrap();
    assert_eq!(pdpt_0 & !0xFFF, capsule_boot::PD_ADDRESS.raw());

    // Comprobar que la primera entrada PD mapea los primeros 2MB con el bit Huge (0x80)
    let pd_0 = mem.read_u64(capsule_boot::PD_ADDRESS).unwrap();
    assert_eq!(pd_0 & 0x83, 0x83, "Bit Present, Writable y Huge 2MB deben estar activos");
}
