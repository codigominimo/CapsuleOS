use capsule_storage::{
    BlockDevice, CowOverlayDisk, MemoryDisk, RawDiskImage, StorageError, VhdFixedDisk,
    DEFAULT_SECTOR_SIZE,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_image_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("capsuleos_{name}_{nanos}.img"))
}

#[test]
fn memory_disk_read_write_roundtrip() {
    let mut disk = MemoryDisk::new(8, DEFAULT_SECTOR_SIZE).expect("disk");
    assert_eq!(disk.sector_size(), DEFAULT_SECTOR_SIZE);
    assert_eq!(disk.sector_count(), 8);
    assert_eq!(disk.capacity_bytes(), 8 * DEFAULT_SECTOR_SIZE as u64);
    assert!(!disk.is_read_only());

    let mut write_buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 2];
    for (i, b) in write_buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    disk.write_sectors(2, 2, &write_buf).expect("write");

    let mut read_buf = vec![0u8; write_buf.len()];
    disk.read_sectors(2, 2, &mut read_buf).expect("read");
    assert_eq!(read_buf, write_buf);

    // Sector no escrito debe permanecer en cero
    let mut sector0 = vec![0xAAu8; DEFAULT_SECTOR_SIZE as usize];
    disk.read_sectors(0, 1, &mut sector0).expect("read sector 0");
    assert!(sector0.iter().all(|&b| b == 0));
}

#[test]
fn memory_disk_bounds_and_buffer_validation() {
    let mut disk = MemoryDisk::new(4, DEFAULT_SECTOR_SIZE).expect("disk");

    let mut small = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    let err = disk.read_sectors(3, 2, &mut small).unwrap_err();
    assert!(matches!(err, StorageError::InvalidBufferSize { .. }));

    let mut buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 2];
    let err = disk.read_sectors(3, 2, &mut buf).unwrap_err();
    assert!(matches!(err, StorageError::OutOfBounds { .. }));

    let err = disk.write_sectors(4, 1, &small).unwrap_err();
    assert!(matches!(err, StorageError::OutOfBounds { .. }));
}

#[test]
fn memory_disk_read_only() {
    let data = vec![0x5Au8; DEFAULT_SECTOR_SIZE as usize * 2];
    let mut disk = MemoryDisk::from_bytes(data.clone(), DEFAULT_SECTOR_SIZE, true).expect("disk");
    assert!(disk.is_read_only());

    let mut out = vec![0u8; data.len()];
    disk.read_sectors(0, 2, &mut out).expect("read");
    assert_eq!(out, data);

    let err = disk.write_sectors(0, 1, &vec![0u8; DEFAULT_SECTOR_SIZE as usize]).unwrap_err();
    assert!(matches!(err, StorageError::ReadOnly));
}

#[test]
fn raw_disk_image_create_open_read_write_flush() {
    let path = temp_image_path("raw_rw");
    let _guard = scopeguard_path(path.clone());

    {
        let mut disk = RawDiskImage::create(&path, 16, DEFAULT_SECTOR_SIZE).expect("create");
        assert_eq!(disk.sector_count(), 16);
        assert_eq!(disk.sector_size(), DEFAULT_SECTOR_SIZE);

        let mut pattern = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 3];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        disk.write_sectors(4, 3, &pattern).expect("write");
        disk.flush().expect("flush");
    }

    {
        let mut disk = RawDiskImage::open(&path, DEFAULT_SECTOR_SIZE).expect("open");
        let mut buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 3];
        disk.read_sectors(4, 3, &mut buf).expect("read");
        for (i, b) in buf.iter().enumerate() {
            assert_eq!(*b, (i as u8).wrapping_mul(7).wrapping_add(3));
        }

        let zeros = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
        let mut first = vec![0xFFu8; DEFAULT_SECTOR_SIZE as usize];
        disk.read_sectors(0, 1, &mut first).expect("read sector 0");
        assert_eq!(first, zeros);
    }
}

#[test]
fn raw_disk_image_open_read_only_rejects_write() {
    let path = temp_image_path("raw_ro");
    let _guard = scopeguard_path(path.clone());

    {
        let mut disk = RawDiskImage::create(&path, 4, DEFAULT_SECTOR_SIZE).expect("create");
        let data = vec![0xCCu8; DEFAULT_SECTOR_SIZE as usize];
        disk.write_sectors(1, 1, &data).expect("seed write");
        disk.flush().expect("flush");
    }

    let mut disk = RawDiskImage::open_read_only(&path, DEFAULT_SECTOR_SIZE).expect("open ro");
    assert!(disk.is_read_only());
    let mut out = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    disk.read_sectors(1, 1, &mut out).expect("read");
    assert!(out.iter().all(|&b| b == 0xCC));

    let err = disk
        .write_sectors(1, 1, &vec![0u8; DEFAULT_SECTOR_SIZE as usize])
        .unwrap_err();
    assert!(matches!(err, StorageError::ReadOnly));
}

struct PathGuard(PathBuf);
impl Drop for PathGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn scopeguard_path(path: PathBuf) -> PathGuard {
    PathGuard(path)
}

#[test]
fn vhd_fixed_create_open_read_write() {
    let path = temp_image_path("vhd_fixed");
    let _guard = scopeguard_path(path.clone());

    {
        let mut disk = VhdFixedDisk::create(&path, 32, DEFAULT_SECTOR_SIZE).expect("create vhd");
        assert_eq!(disk.sector_count(), 32);
        assert_eq!(disk.sector_size(), DEFAULT_SECTOR_SIZE);

        let mut pattern = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 2];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = 0xA0u8.wrapping_add(i as u8);
        }
        disk.write_sectors(10, 2, &pattern).expect("write vhd");
        disk.flush().expect("flush vhd");
    }

    {
        let mut disk = VhdFixedDisk::open(&path, DEFAULT_SECTOR_SIZE).expect("open vhd");
        let mut buf = vec![0u8; DEFAULT_SECTOR_SIZE as usize * 2];
        disk.read_sectors(10, 2, &mut buf).expect("read vhd");
        for (i, b) in buf.iter().enumerate() {
            assert_eq!(*b, 0xA0u8.wrapping_add(i as u8));
        }
    }
}

#[test]
fn vhd_fixed_read_only_rejects_write() {
    let path = temp_image_path("vhd_ro");
    let _guard = scopeguard_path(path.clone());

    {
        let mut disk = VhdFixedDisk::create(&path, 8, DEFAULT_SECTOR_SIZE).expect("create vhd");
        let data = vec![0x3Cu8; DEFAULT_SECTOR_SIZE as usize];
        disk.write_sectors(2, 1, &data).expect("seed");
        disk.flush().expect("flush");
    }

    let mut disk = VhdFixedDisk::open_read_only(&path, DEFAULT_SECTOR_SIZE).expect("open ro vhd");
    let mut out = vec![0u8; DEFAULT_SECTOR_SIZE as usize];
    disk.read_sectors(2, 1, &mut out).expect("read");
    assert!(out.iter().all(|&b| b == 0x3C));

    let err = disk
        .write_sectors(2, 1, &vec![0u8; DEFAULT_SECTOR_SIZE as usize])
        .unwrap_err();
    assert!(matches!(err, StorageError::ReadOnly));
}

#[test]
fn cow_overlay_preserves_base_and_persists_changes_raw() {
    let base_path = temp_image_path("cow_base_raw");
    let ov_path = temp_image_path("cow_overlay_raw");
    let _g1 = scopeguard_path(base_path.clone());
    let _g2 = scopeguard_path(ov_path.clone());

    let sector_size = DEFAULT_SECTOR_SIZE as usize;
    let mut base_seed = vec![0u8; sector_size * 4];
    for (i, b) in base_seed.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7).wrapping_add(0x11);
    }

    {
        let mut base = RawDiskImage::create(&base_path, 16, DEFAULT_SECTOR_SIZE).expect("create base");
        base.write_sectors(2, 4, &base_seed).expect("seed base");
        base.flush().expect("flush base");
    }

    let mut write_buf = vec![0u8; sector_size * 2];
    for (i, b) in write_buf.iter_mut().enumerate() {
        *b = 0xAA ^ (i as u8);
    }

    {
        let base = RawDiskImage::open(&base_path, DEFAULT_SECTOR_SIZE).expect("open base");
        let mut cow = CowOverlayDisk::create(base, &ov_path).expect("create cow");
        cow.write_sectors(3, 2, &write_buf).expect("write cow");

        // Lectura dentro del overlay: mezcla de sector no modificado (LBA2 desde base) y modificados (LBA3-4 overlay)
        let mut mixed = vec![0u8; sector_size * 3];
        cow.read_sectors(2, 3, &mut mixed).expect("read mixed");
        assert_eq!(&mixed[..sector_size], &base_seed[..sector_size], "LBA2 debe leerse de base");
        assert_eq!(&mixed[sector_size..], &write_buf[..], "LBA3-4 deben leerse de overlay");

        cow.flush().expect("flush cow");
    }

    // Base intacta
    {
        let mut base = RawDiskImage::open(&base_path, DEFAULT_SECTOR_SIZE).expect("reopen base");
        let mut buf = vec![0u8; sector_size * 4];
        base.read_sectors(2, 4, &mut buf).expect("read base");
        assert_eq!(buf, base_seed, "La imagen base no debe modificarse por escrituras en overlay");
    }

    // Overlay persistente
    {
        let base = RawDiskImage::open(&base_path, DEFAULT_SECTOR_SIZE).expect("reopen base");
        let mut cow = CowOverlayDisk::open(base, &ov_path).expect("open cow");
        let mut out = vec![0u8; sector_size * 2];
        cow.read_sectors(3, 2, &mut out).expect("read overlay");
        assert_eq!(out, write_buf, "Cambios del overlay deben persistir tras reabrir");
    }
}

#[test]
fn cow_overlay_preserves_base_and_persists_changes_vhd() {
    let base_path = temp_image_path("cow_base_vhd");
    let ov_path = temp_image_path("cow_overlay_vhd");
    let _g1 = scopeguard_path(base_path.clone());
    let _g2 = scopeguard_path(ov_path.clone());

    let sector_size = DEFAULT_SECTOR_SIZE as usize;
    let mut base_seed = vec![0u8; sector_size * 2];
    for (i, b) in base_seed.iter_mut().enumerate() {
        *b = 0x40u8.wrapping_add(i as u8);
    }

    {
        let mut base = VhdFixedDisk::create(&base_path, 32, DEFAULT_SECTOR_SIZE).expect("create base vhd");
        base.write_sectors(5, 2, &base_seed).expect("seed base");
        base.flush().expect("flush base");
    }

    let mut write_buf = vec![0u8; sector_size];
    for (i, b) in write_buf.iter_mut().enumerate() {
        *b = 0xD2 ^ (i as u8);
    }

    {
        let base = VhdFixedDisk::open(&base_path, DEFAULT_SECTOR_SIZE).expect("open base vhd");
        let mut cow = CowOverlayDisk::create(base, &ov_path).expect("create cow");
        cow.write_sectors(5, 1, &write_buf).expect("write cow");
        cow.flush().expect("flush cow");
    }

    {
        let mut base = VhdFixedDisk::open(&base_path, DEFAULT_SECTOR_SIZE).expect("reopen base vhd");
        let mut buf = vec![0u8; sector_size];
        base.read_sectors(5, 1, &mut buf).expect("read base");
        assert_eq!(buf, &base_seed[..sector_size], "Base VHD debe permanecer intacta");
    }

    {
        let base = VhdFixedDisk::open(&base_path, DEFAULT_SECTOR_SIZE).expect("reopen base vhd");
        let mut cow = CowOverlayDisk::open(base, &ov_path).expect("open cow");
        let mut out = vec![0u8; sector_size * 2];
        cow.read_sectors(5, 2, &mut out).expect("read cow");
        assert_eq!(&out[..sector_size], &write_buf[..], "Sector modificado debe venir de overlay");
        assert_eq!(&out[sector_size..], &base_seed[sector_size..], "Sector no modificado debe venir de base");
    }
}

#[test]
fn cow_overlay_rejects_mismatched_base_geometry() {
    let base1 = temp_image_path("cow_geom1");
    let base2 = temp_image_path("cow_geom2");
    let ov = temp_image_path("cow_geom_ov");
    let _g1 = scopeguard_path(base1.clone());
    let _g2 = scopeguard_path(base2.clone());
    let _g3 = scopeguard_path(ov.clone());

    {
        let base = RawDiskImage::create(&base1, 16, DEFAULT_SECTOR_SIZE).expect("create base1");
        let _cow = CowOverlayDisk::create(base, &ov).expect("create cow");
    }
    {
        let base = RawDiskImage::create(&base2, 8, DEFAULT_SECTOR_SIZE).expect("create base2");
        let err = match CowOverlayDisk::open(base, &ov) {
            Ok(_) => panic!("Debe rechazar overlay con geometría distinta"),
            Err(e) => e,
        };
        assert!(matches!(err, StorageError::InvalidImage(_)), "Debe rechazar overlay con geometría distinta");
    }
}
