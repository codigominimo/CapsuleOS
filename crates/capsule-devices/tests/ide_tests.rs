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

#[test]
fn ide_irq_pending_cleared_by_status_not_alt_status() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F7, 0xEC, 1); // IDENTIFY
    assert!(ide.irq_pending(), "IDENTIFY debe dejar IRQ pendiente");

    let _ = ide.read_io(0x3F6, 1); // alt status
    assert!(ide.irq_pending(), "La lectura de alternate status no debe limpiar IRQ");

    let _ = ide.read_io(0x1F7, 1); // status
    assert!(!ide.irq_pending(), "La lectura de status debe limpiar IRQ pendiente");
}

#[test]
fn ide_nien_masks_irq_and_srst_resets_state() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // Generar IRQ pendiente
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F7, 0xEC, 1);
    assert!(ide.irq_pending());

    // nIEN enmascara IRQ pendiente
    ide.write_io(0x3F6, 0x02, 1);
    assert!(!ide.irq_pending(), "nIEN debe enmascarar la IRQ pendiente");
    ide.write_io(0x3F6, 0x00, 1);
    assert!(ide.irq_pending(), "Al limpiar nIEN la IRQ pendiente debe volver a ser visible");

    // SRST resetea estado y limpia IRQ
    ide.write_io(0x3F6, 0x04, 1);
    assert!(!ide.irq_pending(), "SRST debe limpiar la IRQ pendiente");
    let status = ide.read_io(0x1F7, 1);
    assert_eq!(status & (0x40 | 0x10), 0x40 | 0x10, "Tras SRST el dispositivo debe quedar DRDY/DSC si hay disco");
    assert_eq!(status & 0x01, 0, "SRST no debe dejar error");

    // El controlador debe seguir operativo tras SRST
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1); // READ
    let st = ide.read_io(0x1F7, 1);
    assert_eq!(st & 0x08, 0x08, "Tras SRST debe poder ejecutarse READ y activar DRQ");
}

#[test]
fn ide_selecting_absent_slave_updates_status_and_errors_on_io() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // Select slave (no device attached)
    ide.write_io(0x1F6, 0xF0, 1); // DEV=1, LBA=1
    let status = ide.read_io(0x1F7, 1);
    assert_eq!(status & 0x40, 0, "Sin slave, DRDY no debe estar activo");
    assert_eq!(status & 0x01, 0, "Seleccionar unidad ausente no debe poner ERR por si sola");

    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1); // READ
    let st = ide.read_io(0x1F7, 1);
    assert_ne!(st & 0x01, 0, "READ a unidad ausente debe señalizar ERR");
    assert_eq!(st & 0x08, 0, "READ fallido no debe activar DRQ");

    // Volver a master debe restaurar estado listo
    ide.write_io(0x1F6, 0xE0, 1);
    let st_master = ide.read_io(0x1F7, 1);
    assert_eq!(st_master & (0x40 | 0x10), 0x40 | 0x10, "Master presente debe quedar DRDY/DSC");
}

#[test]
fn ide_read_write_out_of_range_lba_sets_error() {
    let disk = MemoryDisk::new(8, DEFAULT_SECTOR_SIZE).expect("disk"); // LBAs 0..7
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // READ fuera de rango (LBA 8)
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 8, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let st = ide.read_io(0x1F7, 1);
    assert_ne!(st & 0x01, 0, "READ fuera de rango debe señalizar ERR");
    assert_eq!(st & 0x08, 0, "READ fuera de rango no debe activar DRQ");
    assert_eq!(ide.read_io(0x1F1, 1) & 0x10, 0x10, "READ fuera de rango debe reportar IDNF");

    // WRITE fuera de rango
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 8, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x30, 1);
    let st = ide.read_io(0x1F7, 1);
    assert_ne!(st & 0x01, 0, "WRITE fuera de rango debe señalizar ERR");
    assert_eq!(st & 0x08, 0, "WRITE fuera de rango no debe activar DRQ");
    assert_eq!(ide.read_io(0x1F1, 1) & 0x10, 0x10, "WRITE fuera de rango debe reportar IDNF");
}

#[test]
fn ide_read_write_requires_lba_mode() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // CHS mode (bit 6 = 0), master
    ide.write_io(0x1F6, 0xA0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1); // READ
    let st = ide.read_io(0x1F7, 1);
    assert_ne!(st & 0x01, 0, "READ en modo CHS debe ser rechazado en esta implementación LBA28");
    assert_eq!(st & 0x08, 0);
    assert_eq!(ide.read_io(0x1F1, 1) & 0x04, 0x04, "Modo no soportado debe reportar ABRT");
}

#[test]
fn ide_new_command_while_drq_active_replaces_pending_transfer() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1); // READ -> DRQ

    let st = ide.read_io(0x1F7, 1); // limpia IRQ pendiente si existiera
    assert_eq!(st & 0x08, 0x08, "READ debe activar DRQ");

    // Emitir nuevo comando con DRQ activo: debe cancelar la transferencia pendiente y ejecutar el nuevo comando.
    ide.write_io(0x1F7, 0xE7, 1); // FLUSH
    assert!(ide.irq_pending(), "FLUSH completado debe dejar condición de IRQ pendiente");
    let st_alt = ide.read_io(0x3F6, 1); // alt status no limpia IRQ
    assert_eq!(st_alt & 0x01, 0, "El nuevo comando debe poder ejecutarse sin ERR si es válido");
    assert_eq!(st_alt & 0x08, 0, "La transferencia PIO anterior debe quedar cancelada (DRQ=0)");
    assert_eq!(ide.read_io(0x1F1, 1), 0x00, "No debe reportar ABRT al reemplazar una transferencia pendiente");

    let st = ide.read_io(0x1F7, 1); // limpia IRQ pendiente
    assert_eq!(st & 0x01, 0);
    assert!(!ide.irq_pending(), "Lectura de status debe limpiar la IRQ pendiente");

    // El dispositivo debe seguir operativo
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1); // READ
    let st = ide.read_io(0x1F7, 1);
    assert_eq!(st & 0x08, 0x08, "Tras cancelar DRQ, el controlador debe aceptar nuevos comandos");
}

#[test]
fn ide_identify_absent_slave_sets_error() {
    let disk = MemoryDisk::new(16, DEFAULT_SECTOR_SIZE).expect("disk");
    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // Seleccionar slave ausente (DEV=1, LBA=1)
    ide.write_io(0x1F6, 0xF0, 1);
    ide.write_io(0x1F7, 0xEC, 1); // IDENTIFY
    let st = ide.read_io(0x1F7, 1);
    assert_ne!(st & 0x01, 0, "IDENTIFY en unidad ausente debe señalizar ERR");
    assert_eq!(st & 0x08, 0, "IDENTIFY fallido no debe activar DRQ");
    assert_eq!(ide.read_io(0x1F1, 1) & 0x04, 0x04, "IDENTIFY en unidad ausente debe reportar ABRT");
}

#[test]
fn ide_sector_count_zero_means_256_sectors_pio() {
    let mut disk = MemoryDisk::new(300, DEFAULT_SECTOR_SIZE).expect("disk");
    let sector_size = DEFAULT_SECTOR_SIZE as usize;

    // Sembrar patrones en el primer y último sector del rango de 256 sectores desde LBA 0
    let mut s0 = vec![0u8; sector_size];
    let mut s255 = vec![0u8; sector_size];
    for i in 0..sector_size {
        s0[i] = (i as u8).wrapping_mul(13).wrapping_add(0x11);
        s255[i] = (i as u8).wrapping_mul(17).wrapping_add(0x22);
    }
    disk.write_sectors(0, 1, &s0).unwrap();
    disk.write_sectors(255, 1, &s255).unwrap();

    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // READ 256 sectores (sector_count=0) desde LBA 0
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 0, 1); // count=0 => 256
    ide.write_io(0x1F3, 0, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let st = ide.read_io(0x1F7, 1);
    assert_eq!(st & 0x01, 0, "READ count=0 (256) no debe fallar");
    assert_eq!(st & 0x08, 0x08, "READ count=0 (256) debe activar DRQ");

    let mut read_back = vec![0u8; sector_size * 256];
    for i in (0..read_back.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        read_back[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(&read_back[..sector_size], &s0[..], "Primer sector (LBA0) incorrecto");
    assert_eq!(
        &read_back[255 * sector_size..256 * sector_size],
        &s255[..],
        "Sector 255 (último del bloque de 256) incorrecto"
    );
    assert_eq!(ide.read_io(0x1F7, 1) & 0x08, 0, "DRQ debe limpiarse tras leer 256 sectores");

    // WRITE 256 sectores a partir de LBA 10 con un patrón verificable (inicio/fin)
    let mut payload = vec![0u8; sector_size * 256];
    for (i, b) in payload.iter_mut().enumerate() {
        // Patrón simple pero único por byte (no necesario que sea complejo)
        *b = (i as u8).wrapping_mul(3) ^ 0x5A;
    }
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 0, 1); // 256 sectores
    ide.write_io(0x1F3, 10, 1); // LBA 10
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x30, 1); // WRITE
    assert_eq!(ide.read_io(0x1F7, 1) & 0x08, 0x08, "WRITE count=0 (256) debe activar DRQ");
    for i in (0..payload.len()).step_by(4) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&payload[i..i + 4]);
        ide.write_io(0x1F0, u32::from_le_bytes(bytes), 4);
    }
    assert_eq!(ide.read_io(0x1F7, 1) & 0x01, 0, "WRITE count=0 (256) no debe reportar error");

    // Verificar releyendo el primer y último sector escritos
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 10, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let mut first = vec![0u8; sector_size];
    for i in (0..first.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        first[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(first, &payload[..sector_size], "Primer sector escrito (LBA10) incorrecto");

    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F3, 0x09, 1); // LBA 265 (0x0109) low
    ide.write_io(0x1F4, 0x01, 1); // LBA mid
    ide.write_io(0x1F5, 0x00, 1); // LBA high
    ide.write_io(0x1F7, 0x20, 1);
    let mut last = vec![0u8; sector_size];
    for i in (0..last.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        last[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(
        last,
        &payload[255 * sector_size..256 * sector_size],
        "Último sector escrito (LBA265) incorrecto"
    );
}

#[test]
fn ide_set_multiple_mode_and_multiple_pio_commands() {
    let mut disk = MemoryDisk::new(32, DEFAULT_SECTOR_SIZE).expect("disk");
    let sector_size = DEFAULT_SECTOR_SIZE as usize;

    let mut src = vec![0u8; sector_size * 2];
    for (i, b) in src.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(5) ^ 0x3C;
    }
    disk.write_sectors(4, 2, &src).unwrap();

    let mut ide = IdeController::new_primary();
    ide.attach_master(Box::new(disk));

    // SET MULTIPLE MODE (0xC6) - compatibilidad básica
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 1, 1);
    ide.write_io(0x1F7, 0xC6, 1);
    let st = ide.read_io(0x1F7, 1);
    assert_eq!(st & 0x01, 0, "SET MULTIPLE MODE no debe reportar error");

    // READ MULTIPLE (0xC4)
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1);
    ide.write_io(0x1F3, 4, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0xC4, 1);
    let st = ide.read_io(0x1F7, 1);
    assert_eq!(st & 0x08, 0x08, "READ MULTIPLE debe activar DRQ");
    let mut out = vec![0u8; sector_size * 2];
    for i in (0..out.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        out[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(out, src, "READ MULTIPLE debe devolver los sectores solicitados");

    // WRITE MULTIPLE (0xC5)
    let mut payload = vec![0u8; sector_size * 2];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = 0xAA ^ (i as u8);
    }
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1);
    ide.write_io(0x1F3, 6, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0xC5, 1);
    assert_eq!(ide.read_io(0x1F7, 1) & 0x08, 0x08, "WRITE MULTIPLE debe activar DRQ");
    for i in (0..payload.len()).step_by(4) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&payload[i..i + 4]);
        ide.write_io(0x1F0, u32::from_le_bytes(bytes), 4);
    }
    assert_eq!(ide.read_io(0x1F7, 1) & 0x01, 0, "WRITE MULTIPLE no debe reportar error");

    // Verificar escritura
    ide.write_io(0x1F6, 0xE0, 1);
    ide.write_io(0x1F2, 2, 1);
    ide.write_io(0x1F3, 6, 1);
    ide.write_io(0x1F4, 0, 1);
    ide.write_io(0x1F5, 0, 1);
    ide.write_io(0x1F7, 0x20, 1);
    let mut verify = vec![0u8; sector_size * 2];
    for i in (0..verify.len()).step_by(4) {
        let v = ide.read_io(0x1F0, 4);
        verify[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    assert_eq!(verify, payload, "WRITE MULTIPLE debe persistir los datos");
}
