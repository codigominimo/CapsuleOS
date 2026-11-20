//! Emulación del controlador de puerto serie UART 16550 estándar de PC (COM1 / 0x3F8).

use std::collections::VecDeque;

pub const COM1_BASE: u16 = 0x3F8;

// Desplazamientos de registros UART estándar
pub const UART_RBR: u16 = 0; // Receiver Buffer Register (lectura, DLAB=0)
pub const UART_THR: u16 = 0; // Transmitter Holding Register (escritura, DLAB=0)
pub const UART_DLL: u16 = 0; // Divisor Latch Low (DLAB=1)
pub const UART_IER: u16 = 1; // Interrupt Enable Register (DLAB=0)
pub const UART_DLM: u16 = 1; // Divisor Latch High (DLAB=1)
pub const UART_IIR: u16 = 2; // Interrupt Identification Register (lectura)
pub const UART_FCR: u16 = 2; // FIFO Control Register (escritura)
pub const UART_LCR: u16 = 3; // Line Control Register (DLAB es el bit 7)
pub const UART_MCR: u16 = 4; // Modem Control Register
pub const UART_LSR: u16 = 5; // Line Status Register
pub const UART_MSR: u16 = 6; // Modem Status Register
pub const UART_SCR: u16 = 7; // Scratch Register

// Bits de LSR (Line Status Register)
pub const LSR_DR: u8 = 1 << 0;   // Data Ready (hay datos para leer en RBR)
pub const LSR_OE: u8 = 1 << 1;   // Overrun Error
pub const LSR_THRE: u8 = 1 << 5; // Transmitter Holding Register Empty (listo para transmitir)
pub const LSR_TEMT: u8 = 1 << 6; // Transmitter Empty

pub struct Uart16550 {
    pub base_port: u16,
    dll: u8,
    dlm: u8,
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    rx_fifo: VecDeque<u8>,
    tx_buffer: Vec<u8>,
}

impl Uart16550 {
    pub fn new(base_port: u16) -> Self {
        Self {
            base_port,
            dll: 0x01, // 115200 baud divisor default
            dlm: 0x00,
            ier: 0x00,
            fcr: 0x00,
            lcr: 0x03, // 8 bits, sin paridad, 1 stop bit (8N1)
            mcr: 0x00,
            scr: 0x00,
            rx_fifo: VecDeque::new(),
            tx_buffer: Vec::new(),
        }
    }

    /// Retorna true si el puerto `port` pertenece a este dispositivo serie [base_port .. base_port + 8).
    #[inline]
    pub fn handles_port(&self, port: u16) -> bool {
        port >= self.base_port && port < self.base_port + 8
    }

    /// Inyecta una secuencia de bytes en el búfer de entrada (host -> guest).
    pub fn queue_input(&mut self, data: &[u8]) {
        for &byte in data {
            self.rx_fifo.push_back(byte);
        }
    }

    /// Retorna los bytes transmitidos por el guest (guest -> host) y limpia el búfer.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx_buffer)
    }

    /// Lee la salida acumulada como cadena UTF-8 sin vaciar el búfer.
    pub fn output_as_string(&self) -> String {
        String::from_utf8_lossy(&self.tx_buffer).to_string()
    }

    /// Lectura de un puerto I/O del UART.
    pub fn read_io(&mut self, port: u16) -> u8 {
        let offset = port - self.base_port;
        let dlab = (self.lcr & 0x80) != 0;

        match offset {
            UART_RBR => {
                if dlab {
                    self.dll
                } else {
                    self.rx_fifo.pop_front().unwrap_or(0)
                }
            }
            UART_IER => {
                if dlab {
                    self.dlm
                } else {
                    self.ier
                }
            }
            UART_IIR => {
                // Si no hay interrupciones pendientes: bit 0 es 1 (No interrupt pending)
                0x01
            }
            UART_LCR => self.lcr,
            UART_MCR => self.mcr,
            UART_LSR => {
                // Siempre listo para transmitir (THRE | TEMT = 0x60)
                let mut lsr = LSR_THRE | LSR_TEMT;
                if !self.rx_fifo.is_empty() {
                    lsr |= LSR_DR; // Datos listos en RBR
                }
                lsr
            }
            UART_MSR => {
                // DSR, CTS, DCD activos (0xB0)
                0xB0
            }
            UART_SCR => self.scr,
            _ => 0xFF,
        }
    }

    /// Escritura en un puerto I/O del UART.
    pub fn write_io(&mut self, port: u16, val: u8) {
        let offset = port - self.base_port;
        let dlab = (self.lcr & 0x80) != 0;

        match offset {
            UART_THR => {
                if dlab {
                    self.dll = val;
                } else {
                    // Escritura en Transmitter Holding Register: carácter enviado
                    self.tx_buffer.push(val);
                }
            }
            UART_IER => {
                if dlab {
                    self.dlm = val;
                } else {
                    self.ier = val;
                }
            }
            UART_FCR => self.fcr = val,
            UART_LCR => self.lcr = val,
            UART_MCR => self.mcr = val,
            UART_SCR => self.scr = val,
            _ => {}
        }
    }
}
