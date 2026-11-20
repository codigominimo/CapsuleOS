//! Controlador ATA/IDE PIO mínimo (canal primario, disco maestro).
//!
//! Implementación simplificada orientada a validación de M5:
//! - PIO LBA28
//! - IDENTIFY DEVICE (0xEC)
//! - READ SECTORS (0x20)
//! - WRITE SECTORS (0x30)
//! - FLUSH CACHE (0xE7)

use capsule_storage::BlockDevice;

pub const PRIMARY_IO_BASE: u16 = 0x1F0;
pub const PRIMARY_IO_END: u16 = 0x1F7; // inclusive
pub const PRIMARY_CTRL: u16 = 0x3F6;

const REG_DATA: u16 = 0;
const REG_ERROR: u16 = 1; // read
const REG_FEATURES: u16 = 1; // write
const REG_SECTOR_COUNT: u16 = 2;
const REG_LBA_LOW: u16 = 3;
const REG_LBA_MID: u16 = 4;
const REG_LBA_HIGH: u16 = 5;
const REG_DRIVE_HEAD: u16 = 6;
const REG_STATUS: u16 = 7; // read
const REG_COMMAND: u16 = 7; // write

const SR_ERR: u8 = 0x01;
const SR_DRQ: u8 = 0x08;
const SR_DSC: u8 = 0x10; // seek complete
const SR_DRDY: u8 = 0x40;
#[allow(dead_code)]
const SR_BSY: u8 = 0x80;

const ERR_ABRT: u8 = 0x04;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_FLUSH_CACHE: u8 = 0xE7;
const CMD_IDENTIFY: u8 = 0xEC;

pub struct IdeDrive {
    disk: Box<dyn BlockDevice>,
}

impl IdeDrive {
    pub fn new(disk: Box<dyn BlockDevice>) -> Self {
        Self { disk }
    }

    pub fn disk(&mut self) -> &mut dyn BlockDevice {
        &mut *self.disk
    }
}

pub struct IdeController {
    pub io_base: u16,
    pub ctrl_port: u16,
    master: Option<IdeDrive>,
    slave: Option<IdeDrive>,

    // Registros de tarea
    error: u8,
    features: u8,
    sector_count: u8,
    lba_low: u8,
    lba_mid: u8,
    lba_high: u8,
    drive_head: u8, // bit 4 = drive select (0 master, 1 slave), bit 6 = LBA
    status: u8,

    // Transferencia PIO
    data_buf: Vec<u8>,
    data_index: usize,
    pio_write: bool,
    sectors_remaining: u16,
    current_lba: u64,
}

impl IdeController {
    pub fn new_primary() -> Self {
        Self {
            io_base: PRIMARY_IO_BASE,
            ctrl_port: PRIMARY_CTRL,
            master: None,
            slave: None,
            error: 0,
            features: 0,
            sector_count: 0,
            lba_low: 0,
            lba_mid: 0,
            lba_high: 0,
            drive_head: 0xE0, // LBA bit set, master
            status: 0,
            data_buf: Vec::new(),
            data_index: 0,
            pio_write: false,
            sectors_remaining: 0,
            current_lba: 0,
        }
    }

    pub fn attach_master(&mut self, disk: Box<dyn BlockDevice>) {
        self.master = Some(IdeDrive::new(disk));
        self.status = SR_DRDY | SR_DSC;
        self.error = 0;
    }

    pub fn attach_slave(&mut self, disk: Box<dyn BlockDevice>) {
        self.slave = Some(IdeDrive::new(disk));
    }

    pub fn has_master(&self) -> bool {
        self.master.is_some()
    }

    pub fn handles_port(&self, port: u16) -> bool {
        (port >= self.io_base && port <= self.io_base + 7) || port == self.ctrl_port
    }

    fn selected_is_slave(&self) -> bool {
        (self.drive_head & 0x10) != 0
    }

    fn selected_drive_present(&self) -> bool {
        if self.selected_is_slave() {
            self.slave.is_some()
        } else {
            self.master.is_some()
        }
    }

    fn selected_drive_mut(&mut self) -> Option<&mut IdeDrive> {
        if self.selected_is_slave() {
            self.slave.as_mut()
        } else {
            self.master.as_mut()
        }
    }

    fn lba28(&self) -> u64 {
        (self.lba_low as u64)
            | ((self.lba_mid as u64) << 8)
            | ((self.lba_high as u64) << 16)
            | (((self.drive_head & 0x0F) as u64) << 24)
    }

    fn sector_count_effective(&self) -> u16 {
        if self.sector_count == 0 {
            256
        } else {
            self.sector_count as u16
        }
    }

    fn set_error_abort(&mut self) {
        self.error = ERR_ABRT;
        self.status = SR_DRDY | SR_DSC | SR_ERR;
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
    }

    fn prepare_read(&mut self, lba: u64, count: u16) {
        let sector_size = match self.selected_drive_mut() {
            Some(d) => d.disk.sector_size() as usize,
            None => {
                self.set_error_abort();
                return;
            }
        };
        let total_bytes = sector_size.saturating_mul(count as usize);
        let mut buf = vec![0u8; total_bytes];

        let res = {
            let drive = self.selected_drive_mut().unwrap();
            drive.disk.read_sectors(lba, count as u32, &mut buf)
        };

        match res {
            Ok(()) => {
                self.data_buf = buf;
                self.data_index = 0;
                self.pio_write = false;
                self.sectors_remaining = count;
                self.current_lba = lba;
                self.status = SR_DRDY | SR_DSC | SR_DRQ;
                self.error = 0;
            }
            Err(_) => self.set_error_abort(),
        }
    }

    fn prepare_write(&mut self, lba: u64, count: u16) {
        let sector_size = match self.selected_drive_mut() {
            Some(d) => d.disk.sector_size() as usize,
            None => {
                self.set_error_abort();
                return;
            }
        };
        self.data_buf = vec![0u8; sector_size.saturating_mul(count as usize)];
        self.data_index = 0;
        self.pio_write = true;
        self.sectors_remaining = count;
        self.current_lba = lba;
        self.status = SR_DRDY | SR_DSC | SR_DRQ;
        self.error = 0;
    }

    fn commit_write(&mut self) {
        let count = self.sectors_remaining as u32;
        let lba = self.current_lba;
        let res = if self.selected_is_slave() {
            if self.slave.is_none() {
                self.set_error_abort();
                return;
            }
            self.slave
                .as_mut()
                .unwrap()
                .disk
                .write_sectors(lba, count, &self.data_buf)
        } else {
            if self.master.is_none() {
                self.set_error_abort();
                return;
            }
            self.master
                .as_mut()
                .unwrap()
                .disk
                .write_sectors(lba, count, &self.data_buf)
        };
        match res {
            Ok(()) => {
                self.status = SR_DRDY | SR_DSC;
                self.error = 0;
                self.data_buf.clear();
                self.data_index = 0;
                self.pio_write = false;
                self.sectors_remaining = 0;
            }
            Err(_) => self.set_error_abort(),
        }
    }

    fn do_identify(&mut self) {
        if !self.selected_drive_present() {
            self.set_error_abort();
            return;
        }
        let (sector_count, sector_size) = {
            let drive = self.selected_drive_mut().unwrap();
            (drive.disk.sector_count(), drive.disk.sector_size())
        };
        if sector_size != 512 {
            // Solo soportamos 512 por ahora para IDENTIFY coherente
            self.set_error_abort();
            return;
        }

        let mut buf = vec![0u8; 512];
        // Word 0: general configuration (non-removable, ATA device)
        put_u16_le(&mut buf, 0 * 2, 0x0040);
        // Word 1/3/6: CHS dummy geometry
        put_u16_le(&mut buf, 1 * 2, 16383);
        put_u16_le(&mut buf, 3 * 2, 16);
        put_u16_le(&mut buf, 6 * 2, 63);
        // Serial number (words 10-19, 20 ASCII chars)
        put_ata_string(&mut buf, 10 * 2, 20, "CAPSULE0001");
        // Firmware revision (words 23-26)
        put_ata_string(&mut buf, 23 * 2, 8, "1.0");
        // Model number (words 27-46)
        put_ata_string(&mut buf, 27 * 2, 40, "CapsuleOS Virtual IDE Disk");
        // Word 49: capabilities (LBA supported)
        put_u16_le(&mut buf, 49 * 2, 0x0200);
        // Word 60-61: total addressable sectors (LBA28)
        let lba28 = sector_count.min(0x0FFF_FFFF) as u32;
        put_u16_le(&mut buf, 60 * 2, (lba28 & 0xFFFF) as u16);
        put_u16_le(&mut buf, 61 * 2, (lba28 >> 16) as u16);
        // Word 83: LBA48 supported flags (no)
        put_u16_le(&mut buf, 83 * 2, 0x0000);
        // Word 100-103: LBA48 total sectors (zero)

        self.data_buf = buf;
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 1;
        self.status = SR_DRDY | SR_DSC | SR_DRQ;
        self.error = 0;
    }

    fn execute_command(&mut self, cmd: u8) {
        match cmd {
            CMD_IDENTIFY => self.do_identify(),
            CMD_READ_SECTORS => {
                if !self.selected_drive_present() {
                    self.set_error_abort();
                    return;
                }
                let lba = self.lba28();
                let count = self.sector_count_effective();
                self.prepare_read(lba, count);
            }
            CMD_WRITE_SECTORS => {
                if !self.selected_drive_present() {
                    self.set_error_abort();
                    return;
                }
                let lba = self.lba28();
                let count = self.sector_count_effective();
                self.prepare_write(lba, count);
            }
            CMD_FLUSH_CACHE => {
                let res = match self.selected_drive_mut() {
                    Some(d) => d.disk.flush(),
                    None => {
                        self.set_error_abort();
                        return;
                    }
                };
                match res {
                    Ok(()) => {
                        self.status = SR_DRDY | SR_DSC;
                        self.error = 0;
                    }
                    Err(_) => self.set_error_abort(),
                }
            }
            _ => {
                // Comando no soportado
                self.set_error_abort();
            }
        }
    }

    pub fn read_io(&mut self, port: u16, size: u8) -> u32 {
        if port == self.ctrl_port {
            // Alt status (no limpia IRQ pendiente en esta implementación)
            return self.status as u32;
        }
        let offset = port.saturating_sub(self.io_base);
        match offset {
            REG_DATA => self.read_data(size),
            REG_ERROR => self.error as u32,
            REG_SECTOR_COUNT => self.sector_count as u32,
            REG_LBA_LOW => self.lba_low as u32,
            REG_LBA_MID => self.lba_mid as u32,
            REG_LBA_HIGH => self.lba_high as u32,
            REG_DRIVE_HEAD => self.drive_head as u32,
            REG_STATUS => {
                // Leer status normalmente limpia la condición de IRQ (no modelada aquí)
                self.status as u32
            }
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, size: u8) {
        if port == self.ctrl_port {
            // Device control (nIEN, SRST). Ignorado en esta etapa.
            let _ = val;
            return;
        }
        let offset = port.saturating_sub(self.io_base);
        match offset {
            REG_DATA => self.write_data(val, size),
            REG_FEATURES => self.features = (val & 0xFF) as u8,
            REG_SECTOR_COUNT => self.sector_count = (val & 0xFF) as u8,
            REG_LBA_LOW => self.lba_low = (val & 0xFF) as u8,
            REG_LBA_MID => self.lba_mid = (val & 0xFF) as u8,
            REG_LBA_HIGH => self.lba_high = (val & 0xFF) as u8,
            REG_DRIVE_HEAD => self.drive_head = (val & 0xFF) as u8,
            REG_COMMAND => self.execute_command((val & 0xFF) as u8),
            _ => {}
        }
    }

    fn read_data(&mut self, size: u8) -> u32 {
        if (self.status & SR_DRQ) == 0 || self.pio_write || self.data_index >= self.data_buf.len() {
            return 0xFFFF_FFFF;
        }
        let n = match size {
            1 => 1,
            2 => 2,
            4 => 4,
            _ => 1,
        };
        let mut val: u32 = 0;
        for i in 0..n {
            let b = if self.data_index < self.data_buf.len() {
                let b = self.data_buf[self.data_index];
                self.data_index += 1;
                b
            } else {
                0
            };
            val |= (b as u32) << (i * 8);
        }

        if self.data_index >= self.data_buf.len() {
            self.status &= !SR_DRQ;
            self.sectors_remaining = 0;
        }
        val
    }

    fn write_data(&mut self, val: u32, size: u8) {
        if (self.status & SR_DRQ) == 0 || !self.pio_write {
            return;
        }
        let n = match size {
            1 => 1,
            2 => 2,
            4 => 4,
            _ => 1,
        };
        for i in 0..n {
            if self.data_index < self.data_buf.len() {
                self.data_buf[self.data_index] = ((val >> (i * 8)) & 0xFF) as u8;
                self.data_index += 1;
            }
        }
        if self.data_index >= self.data_buf.len() {
            self.status &= !SR_DRQ;
            self.commit_write();
        }
    }
}

fn put_u16_le(buf: &mut [u8], offset: usize, val: u16) {
    if offset + 1 < buf.len() {
        buf[offset] = (val & 0xFF) as u8;
        buf[offset + 1] = (val >> 8) as u8;
    }
}

fn put_ata_string(buf: &mut [u8], offset: usize, len: usize, s: &str) {
    // ATA strings: space-padded, each word stores chars swapped (hi, lo)
    let mut tmp = vec![b' '; len];
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(len);
    tmp[..copy_len].copy_from_slice(&bytes[..copy_len]);
    for i in (0..len).step_by(2) {
        if offset + i + 1 >= buf.len() {
            break;
        }
        buf[offset + i] = tmp[i + 1];
        buf[offset + i + 1] = tmp[i];
    }
}
