use capsule_devices::ide::IdeController;
use capsule_storage::{BlockDevice, MemoryDisk, DEFAULT_SECTOR_SIZE};

fn status_drq_drdy_dsc(status: u32) -> bool {
    // DRQ (bit3) + DRDY (bit6) + DSC (bit4)
    (status & 0x58) == 0x58
}

#[test]
fn ide_identify_returns_512_bytes() {
    let disk = MemoryDisk::new(64, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // Select master, LBA mode
    ide.write_io(0x1F6, 0xE0, 1);
    // IDENTIFY
    ide.write_io(0x1F7, 0xEC, 1);

    let status = ide.read_io(0x1F7, 1);
    assert!(status_drq_drdy_dsc(status), "IDENTIFY debe dejar DRQ activo, status=0x{status:02X}");

    let mut data = vec![0u8; 512];
    for i in (0..512).step_by(2) {
        let w = ide.read_io(0x1F0, 2) as u16;
        data[i] = (w & 0xFF) as u8;
        data[i + 1] = (w >> 8) as u8;
    }
    let status_after = ide.read_io(0x1F7, 1);
    assert_eq!(status_after & 0x08, 0, "DRQ debe limpiarse tras leer 512 bytes");

    // Word 49: LBA supported (bit 9)
    let w49 = u16::from_le_bytes([data[49 * 2], data[49 * 2 + 1]]);
    assert_ne!(w49 & 0x0200, 0, "IDENTIFY debe anunciar soporte LBA");
    // LBA28 sector count (words 60-61)
    let lba28 = (u16::from_le_bytes([data[60 * 2], data[60 * 2 + 1]]) as u32)
        | ((u16::from_le_bytes([data[61 * 2], data[61 * 2 + 1]]) as u32) << 16);
    assert_eq!(lba28, 64, "IDENTIFY debe reportar el número de sectores LBA28");
}

#[test]
fn ide_pio_read_and_write_sectors() {
    let mut disk = MemoryDisk::new(32, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut sector0 = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    for (i, b) in sector0.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(3).wrapping_add(1);
    }
    disk.write_sectors(0, 1, &sector0).unwrap();

    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // READ SECTOR 0
    ide.write_io(0x1F6, 0xE0, 1); // master, LBA
    ide.write_io(0x1F2, 1, 1); // count
    ide.write_io(0x1F3, 0, 1); // LBA low
    ide.write_io(0x1F4, 0, 1); // LBA mid
    ide.write_io(0x1F5, 0, 1); // LBA high
    ide.write_io(0x1F7, 0x20, 1); // READ SECTORS

    let status = ide.read_io(0x1F7, 1);
    assert!(status_drq_drdy_dsc(status), "READ debe activar DRQ");
    let mut read_back = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    for i in (0..read_back.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        read_back[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(read_back, sector0, "Datos leídos por PIO deben coincidir");

    // WRITE SECTOR 1
    let mut new_sector = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    for (i, b) in new_sector.iter_mut().enumerate() {
        *b = 0xA5 ^ (i as u8);
    }
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 1, 1); // LBA 1
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x30, 1); // WRITE SECTORS
    let status = ide.read_io(0x1F7, 1);
    assert!(status_drq_drdy_dsc(status), "WRITE debe activar DRQ");
    for i in (0..new_sector.len()).step_by(4) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&new_sector[i..i + 4]);
        ide.write_io(0x1F0, u32::from_le_bytes(bytes), 4);
    }
    let status_done = ide.read_io(0x1F7, 1);
    assert_eq!(status_done & 0x01, 0, "WRITE no debe reportar error");

    // FLUSH
    ide.write_io(0x1F7, 0xE7, 1);
    let status_flush = ide.read_io(0x1F7, 1);
    assert_eq!(status_flush & 0x01, 0, "FLUSH no debe reportar error");

    // Verificar escribiendo leyendo de nuevo por PIO (sector 1)
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 1, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let mut verify = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    for i in (0..verify.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        verify[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(verify, new_sector, "Sector escrito debe persistir en el disco");
}

#[test]
fn ide_error_on_no_device() {
    let mut ide = IdeController::new_primary();
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F7, 0xEC, 1); // IDENTIFY sin disco
    let status = ide.read_io(0x1F7, 1);
    assert_ne!(status & 0x01, 0, "Debe señalizar ERR si no hay dispositivo");
}

#[test]
fn ide_pio_read_write_multiple_sectors() {
    let mut disk = MemoryDisk::new(32, DEFAULT_SECTOR_SIZE).expect("disk");
    let sector_size = DEFAULT_SECTOR_SIZE as usize;

    let mut lba2 = vec![0u8; sector_size];
    let mut lba3 = vec![0u8; sector_size];
    for i in 0..sector_size {
        lba2[i] = (i as u8).wrapping_mul(7).wrapping_add(0x10);
        lba3[i] = (i as u8).wrapping_mul(11).wrapping_add(0x20);
    }
    disk.write_sectors(2, 1, &lba2).unwrap();
    disk.write_sectors(3, 1, &lba3).unwrap();

    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // READ 2 sectors starting at LBA 2
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1); // sector count = 2
    ide.write_io(0x1F3, 2, 1); // LBA low
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let status = ide.read_io(0x1F7, 1);
    assert!(status_drq_drdy_dsc(status), "READ multi-sector debe activar DRQ");

    let mut read_back = vec![0u8; sector_size * 2];
    for i in (0..read_back.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        read_back[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(&read_back[..sector_size], &lba2[..], "Sector LBA2 incorrecto");
    assert_eq!(&read_back[sector_size..], &lba3[..], "Sector LBA3 incorrecto");
    assert_eq!(ide.read_io(0x1F7, 1) & 0x08, 0, "DRQ debe limpiarse tras leer todos los sectores");

    // WRITE 2 sectors starting at LBA 4
    let mut w4 = vec![0u8; sector_size];
    let mut w5 = vec![0u8; sector_size];
    for i in 0..sector_size {
        w4[i] = 0x55 ^ (i as u8);
        w5[i] = 0xAA ^ (i as u8);
    }
    let mut payload = Vec::with_capacity(sector_size * 2);
    payload.extend_from_slice(&w4);
    payload.extend_from_slice(&w5);

    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1);
    ide.write_io(0x1F3, 4, 1); // LBA 4
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x30, 1);
    assert!(status_drq_drdy_dsc(ide.read_io(0x1F7, 1)), "WRITE multi-sector debe activar DRQ");
    for i in (0..payload.len()).step_by(4) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&payload[i..i + 4]);
        ide.write_io(0x1F0, u32::from_le_bytes(bytes), 4);
    }
    assert_eq!(ide.read_io(0x1F7, 1) & 0x01, 0, "WRITE multi-sector no debe reportar error");

    ide.write_io(0x1F7, 0xE7, 1); // flush
    assert_eq!(ide.read_io(0x1F7, 1) & 0x01, 0, "FLUSH no debe reportar error");

    // Verify by reading back 2 sectors
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1);
    ide.write_io(0x1F3, 4, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let mut verify = vec![0u8; sector_size * 2];
    for i in (0..verify.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        verify[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(verify, payload, "Los sectores escritos en multi-sector deben persistir");
}
