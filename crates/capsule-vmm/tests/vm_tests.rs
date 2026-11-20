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

    // Código guest mínimo x86-64:
    // mov eax, 0x00000042  ; B8 42 00 00 00
    // add eax, 0x10        ; 83 C0 10       (EAX = 0x52)
    // inc eax              ; FF C0          (EAX = 0x53)
    // hlt                  ; F4
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

    println!("VM Status: {:?}", vm.status);
    println!("Registers:\n{}", vm.vcpu.regs);
}

#[test]
fn test_vm_io_port_device_dispatch() {
    let config = VmConfig {
        name: "IoTestVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(1),
    };

    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

    // Registrar consola de depuración en el puerto 0xE9
    let debug_console = Box::new(DebugConsoleDevice::new(0xE9));
    vm.add_device(debug_console);

    // Código guest:
    // mov eax, 'H' (0x48)  ; B8 48 00 00 00
    // out 0xE9, al         ; E6 E9
    // mov eax, 'i' (0x69)  ; B8 69 00 00 00
    // out 0xE9, al         ; E6 E9
    // hlt                  ; F4
    let guest_code = [
        0xB8, 0x48, 0x00, 0x00, 0x00,
        0xE6, 0xE9,
        0xB8, 0x69, 0x00, 0x00, 0x00,
        0xE6, 0xE9,
        0xF4,
    ];

    vm.load_code(GuestAddress::new(0), &guest_code).unwrap();
    let exit = vm.run().unwrap();

    assert_eq!(exit, VcpuExit::Halt);
    println!("I/O Test completado con éxito");
}
