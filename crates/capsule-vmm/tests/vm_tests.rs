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

#[test]
fn test_vm_ide_pio_identify_and_read() {
    use capsule_storage::{BlockDevice, MemoryDisk, DEFAULT_SECTOR_SIZE};

    let config = VmConfig {
        name: "IdePioVM".to_string(),
        vcpu_count: 1,
        memory_size: ByteSize::from_mib(2),
    };
    let mut vm = VirtualMachine::new(config).expect("Fallo al crear VM");

    // Disco con patrón conocido en sector 0 (LBA 0)
    let mut disk = MemoryDisk::new(64, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut sector0 = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    for (i, b) in sector0.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7).wrapping_add(0x11);
    }
    disk.write_sectors(0, 1, &sector0).expect("seed sector 0");
    vm.attach_primary_master_disk(Box::new(disk))
        .expect("attach ide disk");

    // --- Fase 1: IDENTIFY (0xEC) y guardar status en 0x0500 ---
    // mov dx,0x1F6; mov al,0xE0; out dx,al
    // mov dx,0x1F7; mov al,0xEC; out dx,al
    // in  al,dx
    // mov ebx,0x0500; mov [ebx],al
    // hlt
    let identify_code = [
        0xBA, 0xF6, 0x01, 0x00, 0x00, // mov edx, 0x1F6
        0xB0, 0xE0, // mov al, 0xE0
        0xEE, // out dx, al
        0xBA, 0xF7, 0x01, 0x00, 0x00, // mov edx, 0x1F7
        0xB0, 0xEC, // mov al, 0xEC
        0xEE, // out dx, al
        0xEC, // in al, dx
        0xBB, 0x00, 0x05, 0x00, 0x00, // mov ebx, 0x0500
        0x88, 0x03, // mov [ebx], al
        0xF4, // hlt
    ];
    vm.load_code(GuestAddress::new(0), &identify_code)
        .expect("load identify");
    vm.set_rip(0);
    let exit = vm.run().expect("run identify");
    assert_eq!(exit, VcpuExit::Halt);
    let status = vm.memory.read_u8(GuestAddress::new(0x0500)).unwrap();
    assert_eq!(status & 0x08, 0x08, "IDENTIFY debe activar DRQ, status=0x{status:02X}");
    assert_eq!(status & 0x01, 0x00, "IDENTIFY no debe reportar error");

    // --- Fase 2: READ SECTORS (LBA 0, count 1) a 0x1000 ---
    // Programar registros tarea y comando 0x20, luego leer 128 dwords de 0x1F0.
    // mov dx,0x1F6; mov al,0xE0; out dx,al
    // mov dx,0x1F2; mov al,0x01; out dx,al
    // mov dx,0x1F3; mov al,0x00; out dx,al
    // mov dx,0x1F4; mov al,0x00; out dx,al
    // mov dx,0x1F5; mov al,0x00; out dx,al
    // mov dx,0x1F7; mov al,0x20; out dx,al
    // mov dx,0x1F0; mov ebx,0x1000; mov ecx,128
    // loop: in eax,dx; mov [ebx],eax; add ebx,4; dec ecx; jnz loop
    // hlt
    let read_code = [
        0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE, // drive/head
        0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE, // sector count = 1
        0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE, // LBA low = 0
        0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE, // LBA mid = 0
        0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE, // LBA high = 0
        0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x20, 0xEE, // command READ SECTORS
        0xBA, 0xF0, 0x01, 0x00, 0x00, // mov edx, 0x1F0 (data)
        0xBB, 0x00, 0x10, 0x00, 0x00, // mov ebx, 0x1000 (buffer)
        0xB9, 0x80, 0x00, 0x00, 0x00, // mov ecx, 128 (dwords)
        // loop at offset 63:
        0xED, // in eax, dx
        0x89, 0x03, // mov [ebx], eax
        0x83, 0xC3, 0x04, // add ebx, 4
        0xFF, 0xC9, // dec ecx
        0x75, 0xF6, // jnz loop (-10)
        0xF4, // hlt
    ];
    vm.load_code(GuestAddress::new(0), &read_code)
        .expect("load read code");
    vm.set_rip(0);
    let exit = vm.run().expect("run read");
    assert_eq!(exit, VcpuExit::Halt);

    let mut buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    vm.memory
        .read_bytes(GuestAddress::new(0x1000), &mut buf)
        .expect("read buffer");
    assert_eq!(
        buf, sector0,
        "El sector leído por ATA PIO debe coincidir con el disco"
    );
}

#[test]
fn test_vm_ide_pio_write_flush_persistence() {
    use capsule_storage::{BlockDevice, RawDiskImage, DEFAULT_SECTOR_SIZE};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("capsuleos_vm_ide_{nanos}.img"));

    let sector1_seed = {
        let mut v = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
        for (i, b) in v.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(5).wrapping_add(0x33);
        }
        v
    };
    let sector2_pattern = {
        let mut v = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
        for (i, b) in v.iter_mut().enumerate() {
            *b = 0xC3u8 ^ (i as u8).wrapping_mul(3);
        }
        v
    };

    {
        let mut disk = RawDiskImage::create(&path, 16, DEFAULT_SECTOR_SIZE).expect("create raw");
        disk.write_sectors(1, 1, &sector1_seed).expect("seed lba1");
        disk.flush().expect("flush seed");
    }

    {
        let config = VmConfig {
            name: "IdePioWriteVM".to_string(),
            vcpu_count: 1,
            memory_size: ByteSize::from_mib(2),
        };
        let mut vm = VirtualMachine::new(config).expect("create vm");
        let disk = RawDiskImage::open(&path, DEFAULT_SECTOR_SIZE).expect("open raw");
        vm.attach_primary_master_disk(Box::new(disk))
            .expect("attach disk");

        // Patrón que el guest escribirá en LBA 2
        vm.memory
            .write_bytes(GuestAddress::new(0x2000), &sector2_pattern)
            .expect("write pattern to guest mem");

        // Guest: READ LBA1 -> 0x1000, WRITE LBA2 <- 0x2000, FLUSH, HLT
        let mut code: Vec<u8> = Vec::new();
        // Select master, LBA
        code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]);
        // READ SECTORS: count=1, lba=1, cmd=0x20
        code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]); // count
        code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]); // lba low
        code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // lba mid
        code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // lba high
        code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x20, 0xEE]); // cmd
        // Read loop: 128 dwords from 0x1F0 -> 0x1000
        code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]); // dx=0x1F0
        code.extend_from_slice(&[0xBB, 0x00, 0x10, 0x00, 0x00]); // ebx=0x1000
        code.extend_from_slice(&[0xB9, 0x80, 0x00, 0x00, 0x00]); // ecx=128
        let read_loop = code.len();
        code.extend_from_slice(&[0xED]); // in eax, dx
        code.extend_from_slice(&[0x89, 0x03]); // mov [ebx], eax
        code.extend_from_slice(&[0x83, 0xC3, 0x04]); // add ebx, 4
        code.extend_from_slice(&[0xFF, 0xC9]); // dec ecx
        let rel = (read_loop as i32) - ((code.len() as i32) + 2);
        code.extend_from_slice(&[0x75, rel as i8 as u8]); // jnz read_loop

        // WRITE SECTORS: count=1, lba=2, cmd=0x30
        code.extend_from_slice(&[0xBA, 0xF6, 0x01, 0x00, 0x00, 0xB0, 0xE0, 0xEE]); // drive
        code.extend_from_slice(&[0xBA, 0xF2, 0x01, 0x00, 0x00, 0xB0, 0x01, 0xEE]); // count
        code.extend_from_slice(&[0xBA, 0xF3, 0x01, 0x00, 0x00, 0xB0, 0x02, 0xEE]); // lba low 2
        code.extend_from_slice(&[0xBA, 0xF4, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // lba mid
        code.extend_from_slice(&[0xBA, 0xF5, 0x01, 0x00, 0x00, 0xB0, 0x00, 0xEE]); // lba high
        code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0x30, 0xEE]); // cmd
        // Write loop: 128 dwords from 0x2000 -> 0x1F0
        code.extend_from_slice(&[0xBA, 0xF0, 0x01, 0x00, 0x00]); // dx=0x1F0
        code.extend_from_slice(&[0xBB, 0x00, 0x20, 0x00, 0x00]); // ebx=0x2000
        code.extend_from_slice(&[0xB9, 0x80, 0x00, 0x00, 0x00]); // ecx=128
        let write_loop = code.len();
        code.extend_from_slice(&[0x8B, 0x03]); // mov eax, [ebx]
        code.extend_from_slice(&[0xEF]); // out dx, eax
        code.extend_from_slice(&[0x83, 0xC3, 0x04]); // add ebx, 4
        code.extend_from_slice(&[0xFF, 0xC9]); // dec ecx
        let rel = (write_loop as i32) - ((code.len() as i32) + 2);
        code.extend_from_slice(&[0x75, rel as i8 as u8]); // jnz write_loop

        // FLUSH CACHE
        code.extend_from_slice(&[0xBA, 0xF7, 0x01, 0x00, 0x00, 0xB0, 0xE7, 0xEE]);
        code.push(0xF4); // hlt
        vm.load_code(GuestAddress::new(0), &code).expect("load code");
        vm.set_rip(0);
        let exit = vm.run().expect("run guest");
        assert_eq!(exit, VcpuExit::Halt);

        let mut read_buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
        vm.memory
            .read_bytes(GuestAddress::new(0x1000), &mut read_buf)
            .expect("read guest buffer");
        assert_eq!(read_buf, sector1_seed, "Guest debe leer LBA1 correctamente");
    }

    {
        let mut disk = RawDiskImage::open(&path, DEFAULT_SECTOR_SIZE).expect("reopen raw");
        let mut lba2 = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
        disk.read_sectors(2, 1, &mut lba2).expect("read lba2");
        assert_eq!(lba2, sector2_pattern, "LBA2 debe persistir tras WRITE+FLUSH y cierre");
    }

    let _ = std::fs::remove_file(&path);
}
