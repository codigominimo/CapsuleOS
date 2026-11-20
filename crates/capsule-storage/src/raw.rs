use crate::device::BlockDevice;
use crate::error::StorageError;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Imagen de disco en formato RAW respaldada por un archivo del host.
#[derive(Debug)]
pub struct RawDiskImage {
    file: File,
    sector_size: u32,
    sector_count: u64,
    read_only: bool,
}

impl RawDiskImage {
    /// Abre una imagen RAW existente en modo lectura/escritura.
    pub fn open<P: AsRef<Path>>(path: P, sector_size: u32) -> Result<Self, StorageError> {
        Self::open_with_options(path, sector_size, false)
    }

    /// Abre una imagen RAW existente en modo solo lectura.
    pub fn open_read_only<P: AsRef<Path>>(path: P, sector_size: u32) -> Result<Self, StorageError> {
        Self::open_with_options(path, sector_size, true)
    }

    fn open_with_options<P: AsRef<Path>>(path: P, sector_size: u32, read_only: bool) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }

        let file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .open(path)?;

        let len = file.metadata()?.len();
        if len == 0 || len % sector_size as u64 != 0 {
            return Err(StorageError::InvalidImage(format!(
                "El tamaño de la imagen ({}) debe ser múltiplo del sector ({}) y no vacío",
                len, sector_size
            )));
        }

        Ok(Self {
            file,
            sector_size,
            sector_count: len / sector_size as u64,
            read_only,
        })
    }

    /// Crea una nueva imagen RAW del tamaño indicado, inicializada a ceros.
    pub fn create<P: AsRef<Path>>(path: P, sector_count: u64, sector_size: u32) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }
        let total_len = sector_count
            .checked_mul(sector_size as u64)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento al calcular tamaño de imagen".to_string()))?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_len)?;

        Ok(Self {
            file,
            sector_size,
            sector_count,
            read_only: false,
        })
    }

    fn validate_range(&self, lba: u64, count: u32, buf_len: usize) -> Result<(u64, usize), StorageError> {
        let expected = (count as usize)
            .checked_mul(self.sector_size as usize)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento de tamaño de E/S".to_string()))?;
        if buf_len != expected {
            return Err(StorageError::InvalidBufferSize {
                expected,
                actual: buf_len,
            });
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

impl BlockDevice for RawDiskImage {
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
