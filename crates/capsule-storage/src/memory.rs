use crate::device::BlockDevice;
use crate::error::StorageError;

/// Dispositivo de bloques respaldado por memoria (útil para pruebas y discos RAM).
#[derive(Debug, Clone)]
pub struct MemoryDisk {
    data: Vec<u8>,
    sector_size: u32,
    read_only: bool,
}

impl MemoryDisk {
    /// Crea un disco en memoria inicializado a ceros.
    pub fn new(sector_count: u64, sector_size: u32) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }
        let total = sector_count
            .checked_mul(sector_size as u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| {
                StorageError::InvalidImage("Tamaño de disco en memoria demasiado grande".to_string())
            })?;

        Ok(Self {
            data: vec![0u8; total],
            sector_size,
            read_only: false,
        })
    }

    /// Crea un disco en memoria a partir de un búfer existente.
    /// El tamaño debe ser múltiplo de `sector_size`.
    pub fn from_bytes(data: Vec<u8>, sector_size: u32, read_only: bool) -> Result<Self, StorageError> {
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector debe ser mayor que cero".to_string(),
            ));
        }
        if data.len() % sector_size as usize != 0 {
            return Err(StorageError::InvalidImage(format!(
                "El tamaño de la imagen ({}) no es múltiplo del sector ({})",
                data.len(),
                sector_size
            )));
        }
        Ok(Self {
            data,
            sector_size,
            read_only,
        })
    }

    fn validate_range(&self, lba: u64, count: u32, buf_len: usize) -> Result<(usize, usize), StorageError> {
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
            sector_count: self.sector_count(),
        })?;
        if end_lba > self.sector_count() {
            return Err(StorageError::OutOfBounds {
                lba,
                count,
                sector_count: self.sector_count(),
            });
        }

        let offset = (lba as usize) * (self.sector_size as usize);
        Ok((offset, expected))
    }
}

impl BlockDevice for MemoryDisk {
    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn sector_count(&self) -> u64 {
        self.data.len() as u64 / self.sector_size as u64
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), StorageError> {
        let (offset, len) = self.validate_range(lba, count, buf.len())?;
        buf.copy_from_slice(&self.data[offset..offset + len]);
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, count: u32, buf: &[u8]) -> Result<(), StorageError> {
        if self.read_only {
            return Err(StorageError::ReadOnly);
        }
        let (offset, len) = self.validate_range(lba, count, buf.len())?;
        self.data[offset..offset + len].copy_from_slice(buf);
        Ok(())
    }
}
