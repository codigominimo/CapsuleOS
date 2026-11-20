use capsule_storage::{
    BlockDevice, MemoryDisk, RawDiskImage, StorageError, VhdFixedDisk, DEFAULT_SECTOR_SIZE,
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
