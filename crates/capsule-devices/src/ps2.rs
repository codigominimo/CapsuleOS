use std::collections::VecDeque;

pub const PS2_DATA_PORT: u16 = 0x60;
pub const PS2_STATUS_PORT: u16 = 0x64; // read: status, write: command

// Status register bits
const STATUS_OUT_FULL: u8 = 1 << 0;
const STATUS_SYSTEM: u8 = 1 << 2;
const STATUS_CMD_DATA: u8 = 1 << 3; // 1 = last write to 64 was command
const STATUS_AUX: u8 = 1 << 5; // 1 = output buffer is from mouse

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputSource {
    Keyboard,
    Mouse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataDest {
    None,
    ControllerCmdByte,
    Mouse,
}

#[derive(Debug, Clone)]
pub struct I8042Ps2Controller {
    // i8042 controller state
    status: u8,
    command_byte: u8,
    data_dest: DataDest,
    last_was_command: bool,
    output: VecDeque<(u8, OutputSource)>,

    // keyboard
    kbd_enabled: bool, // clock/interface enabled (port 1)
    kbd_scanning: bool,
    kbd_expect_data: Option<u8>, // expecting data byte for last kbd command (e.g. ED)

    // mouse
    mouse_enabled: bool, // port 2 enabled
    mouse_streaming: bool,
    mouse_sample_rate: u8,
    mouse_resolution: u8,
    mouse_scaling_2_1: bool,
    mouse_buttons: u8, // bits: 0 left,1 right,2 middle
    mouse_dx_acc: i32,
    mouse_dy_acc: i32,
    _mouse_wheel_acc: i32,
    mouse_expect_data: Option<u8>,
    mouse_device_id: u8,

    // irq latches (level while data pending and enabled)
    irq1: bool,
    irq12: bool,
}

impl Default for I8042Ps2Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl I8042Ps2Controller {
    pub fn new() -> Self {
        let mut c = Self {
            status: STATUS_SYSTEM,
            command_byte: 0x47, // translation, kbd irq, mouse irq, system flag
            data_dest: DataDest::None,
            last_was_command: false,
            output: VecDeque::with_capacity(64),
            kbd_enabled: true,
            kbd_scanning: true,
            kbd_expect_data: None,
            mouse_enabled: true,
            mouse_streaming: false,
            mouse_sample_rate: 100,
            mouse_resolution: 4,
            mouse_scaling_2_1: false,
            mouse_buttons: 0,
            mouse_dx_acc: 0,
            mouse_dy_acc: 0,
            _mouse_wheel_acc: 0,
            mouse_expect_data: None,
            mouse_device_id: 0x00,
            irq1: false,
            irq12: false,
        };
        c.update_irqs();
        c
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, PS2_DATA_PORT | PS2_STATUS_PORT)
    }

    pub fn read_io(&mut self, port: u16, _size: u8) -> u32 {
        match port {
            PS2_STATUS_PORT => self.read_status() as u32,
            PS2_DATA_PORT => self.read_data() as u32,
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, value: u32, _size: u8) {
        let v = (value & 0xFF) as u8;
        match port {
            PS2_STATUS_PORT => self.write_command(v),
            PS2_DATA_PORT => self.write_data(v),
            _ => {}
        }
    }

    fn read_status(&self) -> u8 {
        let mut s = self.status & !(STATUS_OUT_FULL | STATUS_AUX | STATUS_CMD_DATA);
        if let Some(&(_, src)) = self.output.front() {
            s |= STATUS_OUT_FULL;
            if src == OutputSource::Mouse {
                s |= STATUS_AUX;
            }
        }
        if self.last_was_command {
            s |= STATUS_CMD_DATA;
        }
        s
    }

    fn read_data(&mut self) -> u8 {
        if let Some((b, _src)) = self.output.pop_front() {
            self.update_irqs();
            b
        } else {
            0x00
        }
    }

    fn write_command(&mut self, cmd: u8) {
        self.last_was_command = true;
        match cmd {
            0x20 => {
                // read command byte
                self.push_output(self.command_byte, OutputSource::Keyboard);
            }
            0x60 => {
                // write command byte (next data byte)
                self.data_dest = DataDest::ControllerCmdByte;
            }
            0xA7 => {
                // disable mouse port
                self.mouse_enabled = false;
                self.command_byte |= 1 << 5;
                self.update_irqs();
            }
            0xA8 => {
                // enable mouse port
                self.mouse_enabled = true;
                self.command_byte &= !(1 << 5);
                self.update_irqs();
            }
            0xAD => {
                // disable keyboard port
                self.kbd_enabled = false;
                self.command_byte |= 1 << 4;
                self.update_irqs();
            }
            0xAE => {
                // enable keyboard port
                self.kbd_enabled = true;
                self.command_byte &= !(1 << 4);
                self.update_irqs();
            }
            0xAA => {
                // controller self-test
                self.push_output(0x55, OutputSource::Keyboard);
                self.status |= STATUS_SYSTEM;
            }
            0xAB => {
                // test keyboard port
                self.push_output(0x00, OutputSource::Keyboard);
            }
            0xA9 => {
                // test mouse port
                self.push_output(0x00, OutputSource::Keyboard);
            }
            0xD4 => {
                // next byte to mouse
                self.data_dest = DataDest::Mouse;
            }
            _ => {
                // unsupported controller commands ignored
            }
        }
    }

    fn write_data(&mut self, data: u8) {
        self.last_was_command = false;
        match self.data_dest {
            DataDest::ControllerCmdByte => {
                self.command_byte = data;
                self.kbd_enabled = (self.command_byte & (1 << 4)) == 0;
                self.mouse_enabled = (self.command_byte & (1 << 5)) == 0;
                self.data_dest = DataDest::None;
                self.update_irqs();
            }
            DataDest::Mouse => {
                self.data_dest = DataDest::None;
                self.handle_mouse_command(data);
            }
            DataDest::None => {
                self.data_dest = DataDest::None;
                self.handle_keyboard_command(data);
            }
        }
    }

    fn handle_keyboard_command(&mut self, cmd: u8) {
        if let Some(expected) = self.kbd_expect_data.take() {
            match expected {
                0xED | 0xF3 => {
                    let _ = cmd;
                    self.push_kbd(0xFA);
                }
                0xF0 => {
                    // get/set scancode set: if data==0 report current set (2) after ACK in this model
                    if cmd == 0x00 {
                        self.push_kbd(0xFA);
                        self.push_kbd(0x02);
                    } else {
                        let _ = cmd;
                        self.push_kbd(0xFA);
                    }
                }
                _ => {
                    self.push_kbd(0xFA);
                }
            }
            return;
        }
        match cmd {
            0xED => {
                self.push_kbd(0xFA);
                self.kbd_expect_data = Some(0xED);
            }
            0xEE => {
                self.push_kbd(0xEE);
            }
            0xF0 => {
                self.push_kbd(0xFA);
                self.kbd_expect_data = Some(0xF0);
            }
            0xF2 => {
                self.push_kbd(0xFA);
                self.push_kbd(0xAB);
                self.push_kbd(0x83);
            }
            0xF3 => {
                self.push_kbd(0xFA);
                self.kbd_expect_data = Some(0xF3);
            }
            0xF4 => {
                self.kbd_scanning = true;
                self.push_kbd(0xFA);
            }
            0xF5 => {
                self.kbd_scanning = false;
                self.push_kbd(0xFA);
            }
            0xF6 => {
                self.kbd_scanning = true;
                self.push_kbd(0xFA);
            }
            0xFF => {
                self.kbd_scanning = true;
                self.push_kbd(0xFA);
                self.push_kbd(0xAA);
            }
            _ => {
                self.push_kbd(0xFA);
            }
        }
    }

    fn handle_mouse_command(&mut self, cmd: u8) {
        if let Some(expected) = self.mouse_expect_data.take() {
            match expected {
                0xF3 => {
                    self.mouse_sample_rate = cmd;
                    self.push_mouse(0xFA);
                }
                0xE8 => {
                    self.mouse_resolution = cmd & 0x03;
                    self.push_mouse(0xFA);
                }
                _ => {
                    self.push_mouse(0xFA);
                }
            }
            return;
        }
        match cmd {
            0xE6 => {
                self.mouse_scaling_2_1 = false;
                self.push_mouse(0xFA);
            }
            0xE7 => {
                self.mouse_scaling_2_1 = true;
                self.push_mouse(0xFA);
            }
            0xE8 => {
                self.push_mouse(0xFA);
                self.mouse_expect_data = Some(0xE8);
            }
            0xE9 => {
                self.push_mouse(0xFA);
                let mut st = 0u8;
                if self.mouse_buttons & 0x01 != 0 {
                    st |= 1 << 0;
                }
                if self.mouse_buttons & 0x02 != 0 {
                    st |= 1 << 1;
                }
                if self.mouse_buttons & 0x04 != 0 {
                    st |= 1 << 2;
                }
                st |= 1 << 3;
                if self.mouse_scaling_2_1 {
                    st |= 1 << 4;
                }
                if self.mouse_streaming {
                    st |= 1 << 5;
                }
                self.push_mouse(st);
                self.push_mouse(self.mouse_resolution & 0x03);
                self.push_mouse(self.mouse_sample_rate);
            }
            0xEA => {
                self.push_mouse(0xFA);
            }
            0xF0 => {
                self.mouse_streaming = false;
                self.push_mouse(0xFA);
            }
            0xF2 => {
                self.push_mouse(0xFA);
                self.push_mouse(self.mouse_device_id);
            }
            0xF3 => {
                self.push_mouse(0xFA);
                self.mouse_expect_data = Some(0xF3);
            }
            0xF4 => {
                self.mouse_streaming = true;
                self.push_mouse(0xFA);
            }
            0xF5 => {
                self.mouse_streaming = false;
                self.push_mouse(0xFA);
            }
            0xF6 => {
                self.mouse_streaming = false;
                self.mouse_sample_rate = 100;
                self.mouse_resolution = 4;
                self.mouse_scaling_2_1 = false;
                self.push_mouse(0xFA);
            }
            0xFF => {
                self.mouse_streaming = false;
                self.mouse_sample_rate = 100;
                self.mouse_resolution = 4;
                self.mouse_scaling_2_1 = false;
                self.mouse_buttons = 0;
                self.mouse_dx_acc = 0;
                self.mouse_dy_acc = 0;
                self._mouse_wheel_acc = 0;
                self.push_mouse(0xFA);
                self.push_mouse(0xAA);
                self.push_mouse(0x00);
            }
            _ => {
                self.push_mouse(0xFA);
            }
        }
    }

    fn push_output(&mut self, byte: u8, src: OutputSource) {
        if self.output.len() >= 256 {
            let _ = self.output.pop_front();
        }
        self.output.push_back((byte, src));
        self.update_irqs();
    }

    fn push_kbd(&mut self, byte: u8) {
        self.push_output(byte, OutputSource::Keyboard);
    }

    fn push_mouse(&mut self, byte: u8) {
        self.push_output(byte, OutputSource::Mouse);
    }

    fn update_irqs(&mut self) {
        let mut k = false;
        let mut m = false;
        for &(_, src) in &self.output {
            match src {
                OutputSource::Keyboard => k = true,
                OutputSource::Mouse => m = true,
            }
            if k && m {
                break;
            }
        }
        self.irq1 = k && (self.command_byte & 0x01) != 0;
        self.irq12 = m && (self.command_byte & 0x02) != 0;
    }

    pub fn keyboard_irq_pending(&self) -> bool {
        self.irq1
    }

    pub fn mouse_irq_pending(&self) -> bool {
        self.irq12
    }

    /// Encola scancodes de teclado (Set 1) si el escaneo esta habilitado.
    pub fn enqueue_scancodes(&mut self, scancodes: &[u8]) {
        if !self.kbd_scanning {
            return;
        }
        for &b in scancodes {
            self.push_kbd(b);
        }
    }

    /// Actualiza estado de botones y movimiento relativo del raton y genera paquetes si procede.
    pub fn enqueue_mouse_event(&mut self, dx: i32, dy: i32, buttons: u8, wheel: i32) {
        self.mouse_buttons = buttons & 0x07;
        self.mouse_dx_acc = self.mouse_dx_acc.saturating_add(dx);
        self.mouse_dy_acc = self.mouse_dy_acc.saturating_add(dy);
        self._mouse_wheel_acc = self._mouse_wheel_acc.saturating_add(wheel);

        if !self.mouse_enabled || !self.mouse_streaming {
            return;
        }

        loop {
            let dxp = self.mouse_dx_acc.clamp(-255, 255);
            let dyp = self.mouse_dy_acc.clamp(-255, 255);
            if dxp == 0 && dyp == 0 {
                break;
            }
            self.mouse_dx_acc -= dxp;
            self.mouse_dy_acc -= dyp;

            let mut b0: u8 = 0x08; // always 1
            if self.mouse_buttons & 0x01 != 0 {
                b0 |= 0x01;
            }
            if self.mouse_buttons & 0x02 != 0 {
                b0 |= 0x02;
            }
            if self.mouse_buttons & 0x04 != 0 {
                b0 |= 0x04;
            }
            if dxp < 0 {
                b0 |= 0x10;
            }
            // PS/2 mouse Y positivo es hacia arriba; la pantalla entrega dy positivo hacia abajo.
            let ps2_dy = (-dyp).clamp(-255, 255);
            if ps2_dy < 0 {
                b0 |= 0x20;
            }

            self.push_mouse(b0);
            self.push_mouse((dxp as i8) as u8);
            self.push_mouse((ps2_dy as i8) as u8);

            if self.mouse_dx_acc == 0 && self.mouse_dy_acc == 0 {
                break;
            }
            if self.output.len() > 200 {
                break;
            }
        }
    }

    /// Encola un paquete de raton sin movimiento (cambio de botones).
    pub fn enqueue_mouse_button_packet(&mut self) {
        if !self.mouse_enabled || !self.mouse_streaming {
            return;
        }
        let mut b0: u8 = 0x08;
        if self.mouse_buttons & 0x01 != 0 {
            b0 |= 0x01;
        }
        if self.mouse_buttons & 0x02 != 0 {
            b0 |= 0x02;
        }
        if self.mouse_buttons & 0x04 != 0 {
            b0 |= 0x04;
        }
        self.push_mouse(b0);
        self.push_mouse(0);
        self.push_mouse(0);
    }
}
