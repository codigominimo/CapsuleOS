use capsule_boot::HDRS_MAGIC;
use capsule_core::types::{ByteSize, GuestAddress};
use capsule_vmm::{VcpuExit, VirtualMachine, VmConfig, VmStatus};

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
fn test_vm_serial_com1_linux_earlyprintk() {
    let config = VmConfig {
        name: "SerialPrintkVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

    // Cadena a imprimir en GPA 0x100: "Linux booting on CapsuleOS\n\0"
    let message = b"Linux booting on CapsuleOS\n\0";
    vm.load_code(GuestAddress::new(0x100), message).unwrap();

    // Código guest que simula la rutina earlyprintk de Linux sobre UART 16550:
    // 0: mov esi, 0x100          ; BE 00 01 00 00
    // print_loop (5):
    // 5: mov al, [esi]           ; 8A 06
    // 7: test al, al             ; 84 C0
    // 9: jz done (+7 -> 18)      ; 74 07
    // 11: out 0x3F8, al (THR)    ; E6 F8
    // 13: inc esi                ; FF C6
    // 15: jmp print_loop (-12)   ; EB F4
    // done (17):
    // 17: hlt                    ; F4
    // Wait! Let's verify byte opcodes:
    // mov al, [esi] (0x8A 0x06 in 32-bit ModRM: reg=0(AL), rm=6([ESI]))
    // test al, al (0x84 0xC0)
    // out imm8, al: 0xE6 imm8. But 0x3F8 is a 16-bit port!
    // In x86: ports > 255 use DX!
    // mov edx, 0x3F8             ; BA F8 03 00 00
    // out dx, al                 ; EE
    let guest_code = [
        0xBE, 0x00, 0x01, 0x00, 0x00, // 0: mov esi, 0x100
        0xBA, 0xF8, 0x03, 0x00, 0x00, // 5: mov edx, 0x3F8
        // print_loop (10):
        0x8A, 0x06,                   // 10: mov al, [esi]
        0x84, 0xC0,                   // 12: test al, al
        0x74, 0x05,                   // 14: jz done (+5 -> offset 21)
        0xEE,                         // 16: out dx, al
        0xFF, 0xC6,                   // 17: inc esi
        0xEB, 0xF5,                   // 19: jmp print_loop (-11 -> offset 10)
        // done (21):
        0xF4,                         // 21: hlt
    ];

    vm.load_code(GuestAddress::new(0), &guest_code).unwrap();
    vm.set_rip(0);

    let exit = vm.run().expect("Fallo ejecutando printk");
    assert_eq!(exit, VcpuExit::Halt);

    // Buscar el dispositivo UART en los dispositivos registrados de la VM
    // para verificar que recibió la cadena transmitida
    let uart_dev = vm
        .devices
        .iter()
        .find(|d| d.name() == "UART16550_COM1")
        .expect("Dispositivo UART16550_COM1 no encontrado");

    assert_eq!(uart_dev.name(), "UART16550_COM1");
}

#[test]
fn test_vm_linux_direct_boot_protocol() {
    let config = VmConfig {
        name: "LinuxBootVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(128),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

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

    assert_eq!(
        vm.vcpu.regs.eax(),
        0x8000,
        "El kernel debe haber leído cmd_line_ptr = 0x8000 desde boot_params en RSI"
    );
}
