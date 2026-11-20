use capsule_devices::uart16550::*;

#[test]
fn test_uart16550_tx_transmission() {
    let mut uart = Uart16550::new(COM1_BASE);

    // Verificar que LSR indica transmisor listo (THRE | TEMT = 0x60)
    let lsr = uart.read_io(COM1_BASE + UART_LSR);
    assert_eq!(lsr & (LSR_THRE | LSR_TEMT), LSR_THRE | LSR_TEMT);

    // Escribir "CapsuleOS\n" a través de THR (0x3F8)
    for byte in b"CapsuleOS\n" {
        uart.write_io(COM1_BASE + UART_THR, *byte);
    }

    assert_eq!(uart.output_as_string(), "CapsuleOS\n");
    let drained = uart.drain_output();
    assert_eq!(drained, b"CapsuleOS\n");
    assert!(uart.output_as_string().is_empty());
}

#[test]
fn test_uart16550_rx_reception() {
    let mut uart = Uart16550::new(COM1_BASE);

    // Inicialmente no hay datos listos
    let lsr_empty = uart.read_io(COM1_BASE + UART_LSR);
    assert_eq!(lsr_empty & LSR_DR, 0);

    // Inyectar comando en la consola (e.g. "uname -a\n")
    uart.queue_input(b"uname -a\n");

    // Verificar que LSR ahora tiene activo LSR_DR (Data Ready = 1)
    let lsr_ready = uart.read_io(COM1_BASE + UART_LSR);
    assert_ne!(lsr_ready & LSR_DR, 0, "LSR_DR debe estar activo");

    // Leer byte a byte desde RBR (0x3F8)
    let mut received = Vec::new();
    while (uart.read_io(COM1_BASE + UART_LSR) & LSR_DR) != 0 {
        let ch = uart.read_io(COM1_BASE + UART_RBR);
        received.push(ch);
    }

    assert_eq!(received, b"uname -a\n");
    // Al vaciar la cola, LSR_DR debe volver a 0
    let lsr_cleared = uart.read_io(COM1_BASE + UART_LSR);
    assert_eq!(lsr_cleared & LSR_DR, 0);
}

#[test]
fn test_uart16550_dlab_divisor() {
    let mut uart = Uart16550::new(COM1_BASE);

    // Activar DLAB en LCR (bit 7 = 0x80)
    uart.write_io(COM1_BASE + UART_LCR, 0x83);

    // Escribir divisor de baudios (DLL=12, DLM=0 para 9600 baud)
    uart.write_io(COM1_BASE + UART_DLL, 12);
    uart.write_io(COM1_BASE + UART_DLM, 0);

    // Leer con DLAB activo
    assert_eq!(uart.read_io(COM1_BASE + UART_DLL), 12);
    assert_eq!(uart.read_io(COM1_BASE + UART_DLM), 0);

    // Desactivar DLAB
    uart.write_io(COM1_BASE + UART_LCR, 0x03);

    // Ahora 0x3F8 y 0x3F9 acceden a RBR/THR e IER
    assert_eq!(uart.read_io(COM1_BASE + UART_IER), 0);
}
