use capsule_boot::HDRS_MAGIC;
use capsule_core::types::{ByteSize, GuestAddress};
use capsule_vmm::{DebugConsoleDevice, VcpuExit, VirtualMachine, VmConfig, VmStatus};

#[test]
fn test_minimal_vm_lifecycle_and_halt() {
    let config = VmConfig {
        name: "TestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(1),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear la VM");
    assert_eq!(vm.status, VmStatus::Created);

    let guest_code = [
        0xB8, 0x42, 0x00, 0x00, 0x00, // mov eax, 0x42
        0x83, 0xC0, 0x10,             // add eax, 0x10
        0xFF, 0xC0,                   // inc eax
        0xF4,                         // hlt
    ];

    let entry_point = GuestAddress::new(0x0000);
    vm.load_code(entry_point, &guest_code)
        .expect("Fallo al cargar código guest");
    vm.set_rip(0);

    let exit = vm.run().expect("Error durante la ejecución de la VM");

    assert_eq!(exit, VcpuExit::Halt, "El guest debe terminar con HLT");
    assert_eq!(vm.status, VmStatus::Halted, "El estado de la VM debe ser Halted");
    assert_eq!(vm.vcpu.regs.eax(), 0x53, "EAX debe ser 0x42 + 0x10 + 1 = 0x53 (83 decimal)");
    assert_eq!(vm.rip(), 11, "RIP debe haber avanzado exactamente 11 bytes");
}

#[test]
fn test_vm_linux_direct_boot_protocol() {
    let config = VmConfig {
        name: "LinuxBootVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(128),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

    // Construir una imagen bzImage sintética:
    // El punto de entrada del kernel (en 1 MB) ejecuta:
    // mov eax, [rsi + 0x228]  ; 8B 86 28 02 00 00  (cargar cmd_line_ptr desde boot_params)
    // hlt                     ; F4
    let kernel_code = [
        0x8B, 0x86, 0x28, 0x02, 0x00, 0x00, // mov eax, [rsi + 0x228]
        0xF4,                               // hlt
    ];

    let setup_sects = 4u8;
    let header_size = (setup_sects as usize + 1) * 512;
    let mut bzimage = vec![0u8; header_size + kernel_code.len()];
    bzimage[0x1F1] = setup_sects;
    bzimage[0x202..0x206].copy_from_slice(&HDRS_MAGIC.to_le_bytes());
    bzimage[0x206..0x208].copy_from_slice(&0x020Du16.to_le_bytes());
    bzimage[0x211] = 0x01; // LOADED_HIGH
    bzimage[0x214..0x218].copy_from_slice(&0x00100000u32.to_le_bytes()); // code32_start = 1 MB
    bzimage[header_size..].copy_from_slice(&kernel_code);

    let cmdline = "console=ttyS0 quiet";
    let boot_info = vm
        .load_linux(&bzimage, cmdline, None)
        .expect("Fallo en load_linux");

    assert_eq!(boot_info.entry_rip, 0x100000);
    assert_eq!(vm.vcpu.regs.rip, 0x100000);
    assert_eq!(vm.vcpu.regs.rsi, 0x7000, "RSI debe apuntar a la Zero Page (0x7000)");

    let exit = vm.run().expect("Error ejecutando kernel");
    assert_eq!(exit, VcpuExit::Halt);

    // EAX debe contener el puntero cmd_line_ptr (0x8000) leído por el kernel desde boot_params
    assert_eq!(
        vm.vcpu.regs.eax(),
        0x8000,
        "El kernel debe haber leído cmd_line_ptr = 0x8000 desde boot_params en RSI"
    );
}

#[test]
fn test_vm_io_port_device_dispatch() {
    let config = VmConfig {
        name: "IoTestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(1),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

    let debug_console = Box::new(DebugConsoleDevice::new(0xE9));
    vm.add_device(debug_console);

    let guest_code = [
        0xB8, 0x48, 0x00, 0x00, 0x00, // mov eax, 'H'
        0xE6, 0xE9,                   // out 0xE9, al
        0xB8, 0x69, 0x00, 0x00, 0x00, // mov eax, 'i'
        0xE6, 0xE9,                   // out 0xE9, al
        0xF4,                         // hlt
    ];

    vm.load_code(GuestAddress::new(0), &guest_code).unwrap();
    let exit = vm.run().unwrap();

    assert_eq!(exit, VcpuExit::Halt);
}
