use capsule_core::types::GuestAddress;
use capsule_cpu::{CpuExitReason, SimpleVcpu};
use capsule_memory::GuestMemory;

fn create_test_env(code: &[u8]) -> (SimpleVcpu, GuestMemory) {
    let mut mem = GuestMemory::new(64 * 1024).expect("Fallo reservando memoria");
    mem.write_bytes(GuestAddress::new(0), code).expect("Fallo escribiendo código");
    let mut vcpu = SimpleVcpu::new(0);
    vcpu.regs.rip = 0;
    // Configurar pila en la parte alta de la memoria
    vcpu.regs.rsp = 0xF000;
    (vcpu, mem)
}

fn run_until_halt(vcpu: &mut SimpleVcpu, mem: &mut GuestMemory, max_steps: usize) {
    for _ in 0..max_steps {
        match vcpu.step(mem).expect("Fallo en ejecución de instrucción") {
            Some(CpuExitReason::Halt) => return,
            Some(exit) => panic!("Salida inesperada de la CPU: {:?}", exit),
            None => continue,
        }
    }
    panic!("La CPU no alcanzó HLT en {} pasos", max_steps);
}

#[test]
fn test_loop_countdown_jnz() {
    // Cuenta hacia atrás de 10 a 0 y acumula en EAX:
    // mov ecx, 10           ; B9 0A 00 00 00
    // xor eax, eax          ; 31 C0
    // loop_start:
    // add eax, ecx          ; 01 C8
    // dec ecx               ; FF C9
    // jnz loop_start (-6)   ; 75 FA
    // hlt                   ; F4
    let code = [
        0xB9, 0x0A, 0x00, 0x00, 0x00, // mov ecx, 10
        0x31, 0xC0,                   // xor eax, eax
        0x01, 0xC8,                   // add eax, ecx
        0xFF, 0xC9,                   // dec ecx
        0x75, 0xFA,                   // jnz -6 (hacia add eax, ecx)
        0xF4,                         // hlt
    ];

    let (mut vcpu, mut mem) = create_test_env(&code);
    run_until_halt(&mut vcpu, &mut mem, 100);

    // Suma de 1 a 10 = 55 (0x37)
    assert_eq!(vcpu.regs.eax(), 55, "EAX debe ser la suma de 1 a 10 (55)");
    assert_eq!(vcpu.regs.ecx(), 0, "ECX debe ser 0 al salir del bucle");
    assert!(vcpu.regs.zf(), "ZF debe estar activo al finalizar dec ecx a 0");
}

#[test]
fn test_call_ret_stack() {
    // subrutina_add10:
    // add eax, 10          ; 83 C0 0A
    // ret                  ; C3
    // main:
    // mov eax, 5           ; B8 05 00 00 00
    // call subrutina (+4)  ; E8 03 00 00 00
    // hlt                  ; F4
    // (subrutina al final)
    let code = [
        0xB8, 0x05, 0x00, 0x00, 0x00, // 0: mov eax, 5
        0xE8, 0x01, 0x00, 0x00, 0x00, // 5: call +1 -> salto a RIP 11
        0xF4,                         // 10: hlt
        // 11: subrutina
        0x83, 0xC0, 0x0A,             // 11: add eax, 10
        0xC3,                         // 14: ret
    ];

    let (mut vcpu, mut mem) = create_test_env(&code);
    run_until_halt(&mut vcpu, &mut mem, 20);

    // EAX = 5 + 10 = 15
    assert_eq!(vcpu.regs.eax(), 15, "EAX debe ser 15 tras CALL y RET");
    assert_eq!(vcpu.regs.rsp, 0xF000, "RSP debe restaurarse a su valor original tras RET");
    assert_eq!(vcpu.regs.rip, 11, "RIP debe haber detenido en el HLT (después del call)");
}

#[test]
fn test_rex_64bit_operations() {
    // Operaciones de 64 bits utilizando REX.W (0x48):
    // mov rax, 0x0000000100000000 ; 48 B8 00 00 00 00 01 00 00 00
    // mov rdx, 0x0000000200000000 ; 48 BA 00 00 00 00 02 00 00 00
    // add rax, rdx                ; 48 01 D0
    // hlt                         ; F4
    let code = [
        0x48, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x48, 0xBA, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        0x48, 0x01, 0xD0,
        0xF4,
    ];

    let (mut vcpu, mut mem) = create_test_env(&code);
    run_until_halt(&mut vcpu, &mut mem, 20);

    assert_eq!(vcpu.regs.rax, 0x0000000300000000, "RAX debe contener 0x300000000 en 64 bits");
}

#[test]
fn test_fibonacci_sequence() {
    // Calcula Fibonacci(7) = 13:
    // mov ecx, 7        ; B9 07 00 00 00
    // mov eax, 0        ; B8 00 00 00 00 (F0)
    // mov ebx, 1        ; BB 01 00 00 00 (F1)
    // fib_loop:
    // mov edx, eax      ; 89 C2 (temp = a)
    // add edx, ebx      ; 01 DA (temp = a + b)
    // mov eax, ebx      ; 89 D8 (a = b)
    // mov ebx, edx      ; 89 D3 (b = temp)
    // dec ecx           ; FF C9
    // jnz fib_loop (-12); 75 F4
    // hlt               ; F4
    let code = [
        0xB9, 0x07, 0x00, 0x00, 0x00,
        0xB8, 0x00, 0x00, 0x00, 0x00,
        0xBB, 0x01, 0x00, 0x00, 0x00,
        // fib_loop (offset 15)
        0x89, 0xC2,
        0x01, 0xDA,
        0x89, 0xD8,
        0x89, 0xD3,
        0xFF, 0xC9,
        0x75, 0xF4, // -12 bytes de salto
        0xF4,
    ];

    let (mut vcpu, mut mem) = create_test_env(&code);
    run_until_halt(&mut vcpu, &mut mem, 100);

    // F(0)=0, F(1)=1, F(2)=1, F(3)=2, F(4)=3, F(5)=5, F(6)=8, F(7)=13
    assert_eq!(vcpu.regs.eax(), 13, "Fibonacci(7) debe ser 13");
}
