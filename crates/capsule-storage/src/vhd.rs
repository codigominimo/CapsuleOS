use crate::error::StorageError;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::device::BlockDevice;

const VHD_COOKIE: &[u8; 8] = b"conectix";
const VHD_FOOTER_SIZE: usize = 512;
const VHD_VERSION: u32 = 0x0001_0000;
const VHD_TYPE_FIXED: u32 = 2;
const VHD_DISK_DATA_OFFSET_FIXED: u64 = u64::MAX; // 0xFFFFFFFFFFFFFFFF

/// Imagen de disco en formato VHD (Fixed Hard Disk).
#[derive(Debug)]
pub struct VhdFixedDisk {
    file: File,
    sector_size: u32,
    sector_count: u64,
    read_only: bool,
}

impl VhdFixedDisk {
    /// Crea una nueva imagen VHD fija del tamaño indicado, inicializada a ceros.
    pub fn create<P: AsRef<Path>>(path: P, sector_count: u64, sector_size: u32) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }
        let data_len = sector_count
            .checked_mul(sector_size as u64)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento al calcular tamaño de imagen".to_string()))?;
        let total_len = data_len
            .checked_add(VHD_FOOTER_SIZE as u64)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento al calcular tamaño de imagen VHD".to_string()))?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_len)?;

        let (cylinders, heads, sectors_per_track) = chs_geometry(sector_count);
        let footer = build_fixed_footer(data_len, cylinders, heads, sectors_per_track);

        let mut file = file;
        file.seek(SeekFrom::Start(data_len))?;
        file.write_all(&footer)?;
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;

        Ok(Self {
            file,
            sector_size,
            sector_count,
            read_only: false,
        })
    }

    /// Abre una imagen VHD fija existente en modo lectura/escritura.
    pub fn open<P: AsRef<Path>>(path: P, sector_size: u32) -> Result<Self, StorageError> {
        Self::open_with_options(path, sector_size, false)
    }

    /// Abre una imagen VHD fija existente en modo solo lectura.
    pub fn open_read_only<P: AsRef<Path>>(path: P, sector_size: u32) -> Result<Self, StorageError> {
        Self::open_with_options(path, sector_size, true)
    }

    fn open_with_options<P: AsRef<Path>>(path: P, sector_size: u32, read_only: bool) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .open(path)?;

        let len = file.metadata()?.len();
        if len < VHD_FOOTER_SIZE as u64 {
            return Err(StorageError::InvalidImage(
                "Archivo VHD demasiado pequeño para contener footer".to_string(),
            ));
        }

        file.seek(SeekFrom::End(-(VHD_FOOTER_SIZE as i64)))?;
        let mut footer = [0u8; VHD_FOOTER_SIZE];
        file.read_exact(&mut footer)?;

        if &footer[0..8] != VHD_COOKIE {
            return Err(StorageError::InvalidImage(
                "Cookie VHD inválida (se esperaba 'conectix')".to_string(),
            ));
        }

        let disk_type = u32::from_be_bytes([footer[60], footer[61], footer[62], footer[63]]);
        if disk_type != VHD_TYPE_FIXED {
            return Err(StorageError::InvalidImage(format!(
                "Tipo de VHD no soportado: {} (solo fixed=2 en esta etapa)",
                disk_type
            )));
        }

        let data_offset = u64::from_be_bytes([
            footer[16], footer[17], footer[18], footer[19], footer[20], footer[21], footer[22], footer[23],
        ]);
        if data_offset != VHD_DISK_DATA_OFFSET_FIXED {
            return Err(StorageError::InvalidImage(
                "Data offset inválido para VHD fixed".to_string(),
            ));
        }

        let current_size = u64::from_be_bytes([
            footer[48], footer[49], footer[50], footer[51], footer[52], footer[53], footer[54], footer[55],
        ]);
        if current_size == 0 || current_size % sector_size as u64 != 0 {
            return Err(StorageError::InvalidImage(format!(
                "Tamaño de disco VHD ({}) inválido para sector de {} bytes",
                current_size, sector_size
            )));
        }
        if current_size + VHD_FOOTER_SIZE as u64 > len {
            return Err(StorageError::InvalidImage(
                "El tamaño declarado del VHD excede el archivo".to_string(),
            ));
        }

        Ok(Self {
            file,
            sector_size,
            sector_count: current_size / sector_size as u64,
            read_only,
        })
    }

    fn validate_range(&self, lba: u64, count: u32, buf_len: usize) -> Result<(u64, usize), StorageError> {
        let expected = (count as usize)
            .checked_mul(self.sector_size as usize)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento de tamaño de E/S".to_string()))?;
        if buf_len != expected {
            return Err(StorageError::InvalidBufferSize { expected, actual: buf_len });
        }

        let end_lba = lba.checked_add(count as u64).ok_or(StorageError::OutOfBounds {
            lba,
            count,
            sector_count: self.sector_count,
        })?;
        if end_lba > self.sector_count {
            return Err(StorageError::OutOfBounds {
                lba,
                count,
                sector_count: self.sector_count,
            });
        }

        let offset = lba
            .checked_mul(self.sector_size as u64)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento de desplazamiento".to_string()))?;
        Ok((offset, expected))
    }
}

impl BlockDevice for VhdFixedDisk {
    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.sector_count
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), StorageError> {
        let (offset, _) = self.validate_range(lba, count, buf.len())?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)?;
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, count: u32, buf: &[u8]) -> Result<(), StorageError> {
        if self.read_only {
            return Err(StorageError::ReadOnly);
        }
        let (offset, _) = self.validate_range(lba, count, buf.len())?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(buf)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StorageError> {
        self.file.sync_all()?;
        Ok(())
    }
}

fn build_fixed_footer(current_size: u64, cylinders: u16, heads: u8, sectors_per_track: u8) -> [u8; VHD_FOOTER_SIZE] {
    let mut footer = [0u8; VHD_FOOTER_SIZE];
    footer[0..8].copy_from_slice(VHD_COOKIE); // Cookie
    footer[8..12].copy_from_slice(&2u32.to_be_bytes()); // Features (Reserved, 0x00000002)
    footer[12..16].copy_from_slice(&VHD_VERSION.to_be_bytes()); // File Format Version
    footer[16..24].copy_from_slice(&VHD_DISK_DATA_OFFSET_FIXED.to_be_bytes()); // Data Offset
    footer[24..28].copy_from_slice(&vhd_timestamp().to_be_bytes()); // Time Stamp
    footer[28..32].copy_from_slice(b"capo"); // Creator Application
    footer[32..36].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // Creator Version
    footer[36..40].copy_from_slice(b"Wi2k"); // Creator Host OS
    footer[40..48].copy_from_slice(&current_size.to_be_bytes()); // Original Size
    footer[48..56].copy_from_slice(&current_size.to_be_bytes()); // Current Size
    footer[56..58].copy_from_slice(&cylinders.to_be_bytes()); // Disk Geometry cylinders
    footer[58] = heads; // heads
    footer[59] = sectors_per_track; // sectors per track/cylinder
    footer[60..64].copy_from_slice(&VHD_TYPE_FIXED.to_be_bytes()); // Disk Type
    footer[64..68].copy_from_slice(&0u32.to_be_bytes()); // Checksum (calculated below)
    // Unique ID (68..84) left as zero for simplicity
    // Saved State (84) = 0
    // Reserved (85..512) = 0

    let checksum = vhd_checksum(&footer);
    footer[64..68].copy_from_slice(&checksum.to_be_bytes());
    footer
}

fn vhd_checksum(footer: &[u8; VHD_FOOTER_SIZE]) -> u32 {
    let mut sum: u32 = 0;
    for (i, b) in footer.iter().enumerate() {
        if (64..68).contains(&i) {
            continue;
        }
        sum = sum.wrapping_add(*b as u32);
    }
    !sum
}

fn vhd_timestamp() -> u32 {
    // Seconds since 2000-01-01 00:00:00 UTC (VHD epoch). Clamp to 0 on underflow.
    const VHD_EPOCH_OFFSET_SECS: u64 = 946_684_800; // 2000-01-01T00:00:00Z - 1970-01-01T00:00:00Z
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(VHD_EPOCH_OFFSET_SECS).min(u32::MAX as u64) as u32
}

fn chs_geometry(sector_count: u64) -> (u16, u8, u8) {
    // Aproximación CHS básica siguiendo convenciones comunes de VHD.
    let (heads, sectors_per_track): (u8, u8) = if sector_count > 65_535u64 * 16 * 255 {
        (16, 255)
    } else {
        let spt: u8 = 17;
        let c_times_h = sector_count / spt as u64;
        let mut heads = ((c_times_h + 1023) / 1024) as u8;
        if c_times_h >= 1024 * 16 {
            heads = 16;
        }
        heads = heads.clamp(4, 16);
        (heads, spt)
    };

    let cylinders = sector_count / (heads as u64 * sectors_per_track as u64);
    let cylinders = cylinders.clamp(1, u16::MAX as u64) as u16;
    (cylinders, heads.max(1), sectors_per_track)
}
