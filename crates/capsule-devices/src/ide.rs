//! Controlador ATA/IDE PIO mínimo (canal primario/secundario, maestro/esclavo).
//!
//! M5: ATA PIO LBA28 (IDENTIFY, READ/WRITE SECTORS, FLUSH CACHE).
//! M12.2: ATAPI PIO mínimo para CD-ROM (IDENTIFY PACKET DEVICE, PACKET + TEST UNIT READY,
//! REQUEST SENSE, INQUIRY, READ CAPACITY(10), READ(10/12)). Orientado a ISO9660/El Torito.

use capsule_storage::BlockDevice;

pub const PRIMARY_IO_BASE: u16 = 0x1F0;
pub const PRIMARY_IO_END: u16 = 0x1F7; // inclusive
pub const PRIMARY_CTRL: u16 = 0x3F6;
pub const SECONDARY_IO_BASE: u16 = 0x170;
pub const SECONDARY_IO_END: u16 = 0x177;
pub const SECONDARY_CTRL: u16 = 0x376;

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
const ERR_IDNF: u8 = 0x10;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_READ_MULTIPLE: u8 = 0xC4;
const CMD_WRITE_MULTIPLE: u8 = 0xC5;
const CMD_SET_MULTIPLE_MODE: u8 = 0xC6;
const CMD_PACKET: u8 = 0xA0;
const CMD_IDENTIFY_PACKET: u8 = 0xA1;
const CMD_FLUSH_CACHE: u8 = 0xE7;
const CMD_IDENTIFY: u8 = 0xEC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdeDriveKind {
    AtaHdd,
    AtapiCdrom,
}

const ATAPI_SECTOR_SIZE: u32 = 2048;

pub struct IdeDrive {
    disk: Box<dyn BlockDevice>,
    kind: IdeDriveKind,
    atapi_sense_asc: u8,
    atapi_sense_ascq: u8,
}

impl IdeDrive {
    pub fn new_ata(disk: Box<dyn BlockDevice>) -> Self {
        Self {
            disk,
            kind: IdeDriveKind::AtaHdd,
            atapi_sense_asc: 0,
            atapi_sense_ascq: 0,
        }
    }

    pub fn new_atapi(disk: Box<dyn BlockDevice>) -> Self {
        Self {
            disk,
            kind: IdeDriveKind::AtapiCdrom,
            atapi_sense_asc: 0x3A, // medium not present / not ready default-ish? better no sense
            atapi_sense_ascq: 0,
        }
    }

    pub fn disk(&mut self) -> &mut dyn BlockDevice {
        &mut *self.disk
    }

    pub fn kind(&self) -> IdeDriveKind {
        self.kind
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
    sector_count: u8, // ATAPI: interrupt reason
    lba_low: u8,
    lba_mid: u8,   // ATAPI: byte count low
    lba_high: u8,  // ATAPI: byte count high
    drive_head: u8, // bit 4 = drive select (0 master, 1 slave), bit 6 = LBA
    status: u8,

    // Control de dispositivo / interrupciones (modelo simplificado)
    nien: bool,        // Device Control bit 1: deshabilita INTRQ cuando está activo
    irq_pending: bool, // Condición de IRQ pendiente (se limpia al leer status 0x1F7)

    // Transferencia PIO
    data_buf: Vec<u8>,
    data_index: usize,
    pio_write: bool,
    sectors_remaining: u16,
    current_lba: u64,

    // ATAPI
    atapi_packet: [u8; 12],
    atapi_packet_idx: usize,
    atapi_packet_pending: bool,
}

impl IdeController {
    pub fn new_primary() -> Self {
        Self::new(PRIMARY_IO_BASE, PRIMARY_CTRL)
    }

    pub fn new_secondary() -> Self {
        Self::new(SECONDARY_IO_BASE, SECONDARY_CTRL)
    }

    fn new(io_base: u16, ctrl_port: u16) -> Self {
        Self {
            io_base,
            ctrl_port,
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
            nien: false,
            irq_pending: false,
            data_buf: Vec::new(),
            data_index: 0,
            pio_write: false,
            sectors_remaining: 0,
            current_lba: 0,
            atapi_packet: [0u8; 12],
            atapi_packet_idx: 0,
            atapi_packet_pending: false,
        }
    }

    pub fn attach_master(&mut self, disk: Box<dyn BlockDevice>) {
        self.master = Some(IdeDrive::new_ata(disk));
        self.status = SR_DRDY | SR_DSC;
        self.error = 0;
        self.irq_pending = false;
        self.reset_atapi_state();
    }

    pub fn attach_master_atapi(&mut self, disk: Box<dyn BlockDevice>) {
        self.master = Some(IdeDrive::new_atapi(disk));
        self.status = SR_DRDY | SR_DSC;
        self.error = 0;
        self.irq_pending = false;
        self.reset_atapi_state();
    }

    pub fn attach_slave(&mut self, disk: Box<dyn BlockDevice>) {
        self.slave = Some(IdeDrive::new_ata(disk));
    }

    pub fn attach_slave_atapi(&mut self, disk: Box<dyn BlockDevice>) {
        self.slave = Some(IdeDrive::new_atapi(disk));
    }

    pub fn has_master(&self) -> bool {
        self.master.is_some()
    }

    /// Indica si hay una condición de IRQ pendiente y no enmascarada por nIEN.
    pub fn irq_pending(&self) -> bool {
        self.irq_pending && !self.nien
    }

    pub fn handles_port(&self, port: u16) -> bool {
        (port >= self.io_base && port <= self.io_base + 7) || port == self.ctrl_port
    }

    fn reset_atapi_state(&mut self) {
        self.atapi_packet = [0u8; 12];
        self.atapi_packet_idx = 0;
        self.atapi_packet_pending = false;
    }

    fn software_reset(&mut self) {
        self.error = 0;
        self.features = 0;
        self.sector_count = 0;
        self.lba_low = 0;
        self.lba_mid = 0;
        self.lba_high = 0;
        self.drive_head = 0xE0;
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
        self.current_lba = 0;
        self.irq_pending = false;
        self.reset_atapi_state();
        self.status = if self.selected_drive_present() {
            SR_DRDY | SR_DSC
        } else {
            0
        };
    }

    fn refresh_selected_drive_status(&mut self) {
        // Si hay una transferencia PIO en curso (DRQ), no alterar el estado.
        if (self.status & SR_DRQ) != 0 {
            return;
        }
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
        self.current_lba = 0;
        self.error = 0;
        self.irq_pending = false;
        self.reset_atapi_state();
        if !self.selected_drive_present() {
            self.status = 0;
            self.sector_count = 0;
            self.lba_low = 0;
            self.lba_mid = 0;
            self.lba_high = 0;
            return;
        }
        self.status = SR_DRDY | SR_DSC;
        if self.selected_drive_kind() == Some(IdeDriveKind::AtapiCdrom) {
            // ATAPI signature after reset/select (simplified).
            self.sector_count = 0x01;
            self.lba_low = 0x01;
            self.lba_mid = 0x14;
            self.lba_high = 0xEB;
            self.error = 0;
        } else {
            self.sector_count = 0;
            self.lba_low = 0;
            self.lba_mid = 0;
            self.lba_high = 0;
        }
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

    fn selected_drive_kind(&self) -> Option<IdeDriveKind> {
        let d = if self.selected_is_slave() { self.slave.as_ref() } else { self.master.as_ref() };
        d.map(|x| x.kind)
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

    fn set_error(&mut self, err: u8) {
        self.error = err;
        self.status = SR_DRDY | SR_DSC | SR_ERR;
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
        self.irq_pending = true;
    }

    fn set_error_abort(&mut self) {
        self.set_error(ERR_ABRT);
    }

    fn set_error_idnf(&mut self) {
        self.set_error(ERR_IDNF);
    }

    fn validate_lba28_request(&mut self, lba: u64, count: u16) -> bool {
        // Requiere modo LBA para comandos de acceso por sectores en esta implementación.
        if (self.drive_head & 0x40) == 0 {
            self.set_error_abort();
            return false;
        }
        let Some(drive) = self.selected_drive_mut() else {
            self.set_error_abort();
            return false;
        };
        let end = lba.saturating_add(count as u64);
        if lba > 0x0FFF_FFFF || end > 0x1000_0000 || end > drive.disk.sector_count() {
            self.set_error_idnf();
            return false;
        }
        true
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
                self.irq_pending = true;
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
        self.irq_pending = true;
    }

    fn commit_write(&mut self) {
        let count = self.sectors_remaining as u32;
        let lba = self.current_lba;
        let res = if self.selected_is_slave() {
            let Some(drive) = self.slave.as_mut() else {
                self.set_error_abort();
                return;
            };
            drive.disk.write_sectors(lba, count, &self.data_buf)
        } else {
            let Some(drive) = self.master.as_mut() else {
                self.set_error_abort();
                return;
            };
            drive.disk.write_sectors(lba, count, &self.data_buf)
        };
        match res {
            Ok(()) => {
                self.status = SR_DRDY | SR_DSC;
                self.error = 0;
                self.data_buf.clear();
                self.data_index = 0;
                self.pio_write = false;
                self.sectors_remaining = 0;
                self.irq_pending = true;
            }
            Err(_) => self.set_error_abort(),
        }
    }

    fn do_identify(&mut self) {
        if !self.selected_drive_present() {
            self.set_error_abort();
            return;
        }
        if self.selected_drive_kind() == Some(IdeDriveKind::AtapiCdrom) {
            // ATA IDENTIFY on ATAPI device: abort with signature (simplified).
            self.set_error_abort();
            self.lba_mid = 0x14;
            self.lba_high = 0xEB;
            self.sector_count = 0x01;
            self.lba_low = 0x01;
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
        self.irq_pending = true;
    }

    fn do_identify_packet(&mut self) {
        if self.selected_drive_kind() != Some(IdeDriveKind::AtapiCdrom) {
            self.set_error_abort();
            return;
        }
        let mut buf = vec![0u8; 512];
        // Word 0: ATAPI device, CD-ROM (5), removable, packet size 12 bytes (0)
        put_u16_le(&mut buf, 0 * 2, 0x8580);
        put_ata_string(&mut buf, 10 * 2, 20, "CAPSULEATAPI1");
        put_ata_string(&mut buf, 23 * 2, 8, "1.0");
        put_ata_string(&mut buf, 27 * 2, 40, "CapsuleOS Virtual ATAPI CDROM");
        put_u16_le(&mut buf, 49 * 2, 0x0200);
        self.data_buf = buf;
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 1;
        self.status = SR_DRDY | SR_DSC | SR_DRQ;
        self.error = 0;
        self.irq_pending = true;
    }

    fn atapi_set_sense(&mut self, asc: u8, ascq: u8) {
        if let Some(d) = self.selected_drive_mut() {
            d.atapi_sense_asc = asc;
            d.atapi_sense_ascq = ascq;
        }
    }

    fn atapi_clear_sense(&mut self) {
        self.atapi_set_sense(0, 0);
    }

    fn atapi_packet_error(&mut self, sense_key: u8, asc: u8, ascq: u8) {
        self.atapi_set_sense(asc, ascq);
        self.error = (sense_key & 0x0F) << 4;
        self.status = SR_DRDY | SR_DSC | SR_ERR;
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
        self.atapi_packet_pending = false;
        self.sector_count = 0x03; // command, release, io=0
        self.lba_mid = 0;
        self.lba_high = 0;
        self.irq_pending = true;
    }

    fn atapi_begin_packet(&mut self) {
        if self.selected_drive_kind() != Some(IdeDriveKind::AtapiCdrom) {
            self.set_error_abort();
            return;
        }
        self.atapi_packet_pending = true;
        self.atapi_packet_idx = 0;
        self.atapi_packet = [0u8; 12];
        self.data_buf = vec![0u8; 12];
        self.data_index = 0;
        self.pio_write = true;
        self.sectors_remaining = 1;
        self.status = SR_DRDY | SR_DSC | SR_DRQ;
        self.error = 0;
        self.sector_count = 0x01; // command phase
        let byte_count = 12u16;
        self.lba_mid = (byte_count & 0xFF) as u8;
        self.lba_high = (byte_count >> 8) as u8;
        self.irq_pending = false;
    }

    fn atapi_complete_data(&mut self, data: Vec<u8>) {
        let bc = (data.len().min(u16::MAX as usize) as u16).max(1);
        self.data_buf = data;
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 1;
        self.status = SR_DRDY | SR_DSC | SR_DRQ;
        self.error = 0;
        self.sector_count = 0x02; // IO=0, release=1, CoD=0 data
        self.lba_mid = (bc & 0xFF) as u8;
        self.lba_high = (bc >> 8) as u8;
        self.irq_pending = true;
    }

    fn atapi_complete_no_data(&mut self) {
        self.status = SR_DRDY | SR_DSC;
        self.error = 0;
        self.data_buf.clear();
        self.data_index = 0;
        self.pio_write = false;
        self.sectors_remaining = 0;
        self.sector_count = 0x03; // IO=0, release, command
        self.lba_mid = 0;
        self.lba_high = 0;
        self.irq_pending = true;
    }

    fn process_atapi_packet(&mut self) {
        let pkt = self.atapi_packet;
        let op = pkt[0];
        match op {
            0x00 => { // TEST UNIT READY
                self.atapi_clear_sense();
                self.atapi_complete_no_data();
            }
            0x03 => { // REQUEST SENSE
                let alloc = pkt[4] as usize;
                let (asc, ascq) = self.selected_drive_mut().map(|d| (d.atapi_sense_asc, d.atapi_sense_ascq)).unwrap_or((0, 0));
                let mut data = vec![0u8; 18];
                data[0] = 0x70;
                data[2] = if asc == 0 && ascq == 0 { 0x00 } else { 0x06 }; // no sense / unit attention-ish
                data[7] = 10;
                data[12] = asc;
                data[13] = ascq;
                let len = alloc.max(1).min(data.len());
                data.truncate(len);
                self.atapi_complete_data(data);
            }
            0x12 => { // INQUIRY
                let alloc = pkt[4] as usize;
                let mut data = vec![0u8; 36];
                data[0] = 0x05; // CD-ROM
                data[1] = 0x80; // removable
                data[2] = 0x00;
                data[3] = 0x02;
                data[4] = 31;
                write_scsi_ascii(&mut data, 8, 8, "CAPSULE");
                write_scsi_ascii(&mut data, 16, 16, "ATAPI CDROM");
                write_scsi_ascii(&mut data, 32, 4, "1.0");
                let len = alloc.max(1).min(data.len());
                data.truncate(len);
                self.atapi_clear_sense();
                self.atapi_complete_data(data);
            }
            0x1A | 0x5A => { // MODE SENSE(6/10) - minimal
                let alloc = if op == 0x1A { pkt[4] as usize } else { u16::from_be_bytes([pkt[7], pkt[8]]) as usize };
                let mut data = if op == 0x1A { vec![0u8; 8] } else { vec![0u8; 8] };
                data[0] = if op == 0x1A { 7 } else { 0 }; // mode data length-ish
                let len = alloc.max(1).min(data.len());
                data.truncate(len);
                self.atapi_clear_sense();
                self.atapi_complete_data(data);
            }
            0x25 => { // READ CAPACITY(10)
                let (sectors, ss) = self.selected_drive_mut().map(|d| (d.disk.sector_count(), d.disk.sector_size())).unwrap_or((0, ATAPI_SECTOR_SIZE));
                let last_lba = sectors.saturating_sub(1).min(u32::MAX as u64) as u32;
                let mut data = vec![0u8; 8];
                data[0..4].copy_from_slice(&last_lba.to_be_bytes());
                data[4..8].copy_from_slice(&ss.to_be_bytes());
                self.atapi_clear_sense();
                self.atapi_complete_data(data);
            }
            0x28 | 0xA8 => { // READ(10/12)
                let (lba, count) = if op == 0x28 {
                    (u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]) as u64,
                     u16::from_be_bytes([pkt[7], pkt[8]]) as u32)
                } else {
                    (u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]) as u64,
                     u32::from_be_bytes([pkt[6], pkt[7], pkt[8], pkt[9]]))
                };
                let count = if count == 0 { 0 } else { count };
                let (sector_size, sector_count) = self.selected_drive_mut().map(|d| (d.disk.sector_size(), d.disk.sector_count())).unwrap_or((ATAPI_SECTOR_SIZE, 0));
                if count == 0 {
                    self.atapi_clear_sense();
                    self.atapi_complete_no_data();
                    return;
                }
                let end = lba.saturating_add(count as u64);
                if sector_size != ATAPI_SECTOR_SIZE || end > sector_count {
                    self.atapi_packet_error(0x05, 0x21, 0x00); // illegal request / LBA out of range
                    return;
                }
                let byte_len = (count as usize).saturating_mul(sector_size as usize);
                let mut data = vec![0u8; byte_len];
                let res = self.selected_drive_mut().unwrap().disk.read_sectors(lba, count, &mut data);
                match res {
                    Ok(()) => {
                        self.atapi_clear_sense();
                        self.atapi_complete_data(data);
                    }
                    Err(_) => self.atapi_packet_error(0x03, 0x11, 0x00), // medium error / read error
                }
            }
            0x43 => { // READ TOC - minimal, return empty/invalid for now
                self.atapi_packet_error(0x05, 0x24, 0x00);
            }
            _ => {
                self.atapi_packet_error(0x05, 0x20, 0x00); // illegal request / invalid command
            }
        }
    }

    fn execute_command(&mut self, cmd: u8) {
        // Una nueva orden inicia un nuevo ciclo de comando; limpiar IRQ pendiente previa.
        self.irq_pending = false;

        // Si hay una transferencia PIO pendiente, el nuevo comando la reemplaza.
        if (self.status & SR_DRQ) != 0 {
            self.status &= !SR_DRQ;
            self.data_buf.clear();
            self.data_index = 0;
            self.pio_write = false;
            self.sectors_remaining = 0;
            self.atapi_packet_pending = false;
        }

        match cmd {
            CMD_IDENTIFY => self.do_identify(),
            CMD_IDENTIFY_PACKET => self.do_identify_packet(),
            CMD_PACKET => self.atapi_begin_packet(),
            CMD_READ_SECTORS | CMD_READ_MULTIPLE => {
                if self.selected_drive_kind() != Some(IdeDriveKind::AtaHdd) {
                    self.set_error_abort();
                    return;
                }
                if !self.selected_drive_present() {
                    self.set_error_abort();
                    return;
                }
                let lba = self.lba28();
                let count = self.sector_count_effective();
                if !self.validate_lba28_request(lba, count) {
                    return;
                }
                self.prepare_read(lba, count);
            }
            CMD_WRITE_SECTORS | CMD_WRITE_MULTIPLE => {
                if self.selected_drive_kind() != Some(IdeDriveKind::AtaHdd) {
                    self.set_error_abort();
                    return;
                }
                if !self.selected_drive_present() {
                    self.set_error_abort();
                    return;
                }
                let lba = self.lba28();
                let count = self.sector_count_effective();
                if !self.validate_lba28_request(lba, count) {
                    return;
                }
                self.prepare_write(lba, count);
            }
            CMD_FLUSH_CACHE => {
                if self.selected_drive_kind() != Some(IdeDriveKind::AtaHdd) {
                    self.set_error_abort();
                    return;
                }
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
                        self.irq_pending = true;
                    }
                    Err(_) => self.set_error_abort(),
                }
            }
            CMD_SET_MULTIPLE_MODE => {
                if !self.selected_drive_present() || self.selected_drive_kind() != Some(IdeDriveKind::AtaHdd) {
                    self.set_error_abort();
                    return;
                }
                self.status = SR_DRDY | SR_DSC;
                self.error = 0;
                self.irq_pending = true;
            }
            _ => {
                self.set_error_abort();
            }
        }
    }

    pub fn read_io(&mut self, port: u16, size: u8) -> u32 {
        if port == self.ctrl_port {
            // Alternate status: no limpia la condición de IRQ pendiente
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
                // Leer status limpia la condición de IRQ pendiente (modelo simplificado)
                let s = self.status as u32;
                self.irq_pending = false;
                s
            }
            _ => 0xFF,
        }
    }

    pub fn write_io(&mut self, port: u16, val: u32, size: u8) {
        if port == self.ctrl_port {
            // Device Control: bit 1 = nIEN, bit 2 = SRST
            self.nien = (val & 0x02) != 0;
            if (val & 0x04) != 0 {
                self.software_reset();
            }
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
            REG_DRIVE_HEAD => {
                self.drive_head = (val & 0xFF) as u8;
                self.refresh_selected_drive_status();
            }
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
            if self.selected_drive_kind() == Some(IdeDriveKind::AtapiCdrom) && !self.atapi_packet_pending {
                // ATAPI data phase complete -> status phase
                self.status = SR_DRDY | SR_DSC;
                self.sector_count = 0x03;
                self.lba_mid = 0;
                self.lba_high = 0;
                self.irq_pending = true;
            }
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
            if self.atapi_packet_pending {
                self.atapi_packet.copy_from_slice(&self.data_buf[..12]);
                self.atapi_packet_pending = false;
                self.data_buf.clear();
                self.data_index = 0;
                self.pio_write = false;
                self.sectors_remaining = 0;
                self.process_atapi_packet();
            } else {
                self.commit_write();
            }
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

fn write_scsi_ascii(buf: &mut [u8], offset: usize, len: usize, s: &str) {
    if offset >= buf.len() { return; }
    let end = (offset + len).min(buf.len());
    for b in &mut buf[offset..end] { *b = b' '; }
    let bytes = s.as_bytes();
    let n = bytes.len().min(end - offset);
    buf[offset..offset + n].copy_from_slice(&bytes[..n]);
}
