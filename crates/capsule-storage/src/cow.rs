use crate::device::BlockDevice;
use crate::error::StorageError;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const COW_MAGIC: &[u8; 8] = b"CAPSCOW1";
const COW_VERSION: u32 = 1;
const COW_HEADER_SIZE: u64 = 64;

/// Disco de overlay Copy-on-Write (COW).
///
/// Las lecturas se resuelven desde el overlay si el sector fue escrito; en caso
/// contrario se leen del disco base. Las escrituras siempre van al overlay, por lo
/// que la imagen base no se modifica.
#[derive(Debug)]
pub struct CowOverlayDisk<B: BlockDevice> {
    base: B,
    file: File,
    sector_size: u32,
    sector_count: u64,
    bitmap_offset: u64,
    data_offset: u64,
    bitmap: Vec<u8>,
    read_only: bool,
}

impl<B: BlockDevice> CowOverlayDisk<B> {
    /// Crea un nuevo overlay COW vacío para `base`.
    pub fn create<P: AsRef<Path>>(base: B, path: P) -> Result<Self, StorageError> {
        let sector_size = base.sector_size();
        let sector_count = base.sector_count();
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector del disco base debe ser mayor que cero".to_string(),
            ));
        }

        let bitmap_len = bitmap_len_bytes(sector_count);
        let bitmap_offset = COW_HEADER_SIZE;
        let data_offset = align_up(bitmap_offset + bitmap_len as u64, sector_size as u64);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(data_offset)?;

        let mut header = [0u8; COW_HEADER_SIZE as usize];
        header[0..8].copy_from_slice(COW_MAGIC);
        header[8..12].copy_from_slice(&COW_VERSION.to_le_bytes());
        header[12..16].copy_from_slice(&sector_size.to_le_bytes());
        header[16..24].copy_from_slice(&sector_count.to_le_bytes());
        header[24..32].copy_from_slice(&bitmap_offset.to_le_bytes());
        header[32..40].copy_from_slice(&data_offset.to_le_bytes());
        // 40..64 reservados = 0
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header)?;
        file.sync_all()?;

        Ok(Self {
            base,
            file,
            sector_size,
            sector_count,
            bitmap_offset,
            data_offset,
            bitmap: vec![0u8; bitmap_len],
            read_only: false,
        })
    }

    /// Abre un overlay COW existente asociado a `base`.
    pub fn open<P: AsRef<Path>>(base: B, path: P) -> Result<Self, StorageError> {
        Self::open_with_options(base, path, false)
    }

    /// Abre un overlay COW existente en modo solo lectura.
    pub fn open_read_only<P: AsRef<Path>>(base: B, path: P) -> Result<Self, StorageError> {
        Self::open_with_options(base, path, true)
    }

    fn open_with_options<P: AsRef<Path>>(base: B, path: P, read_only: bool) -> Result<Self, StorageError> {
        let sector_size = base.sector_size();
        let sector_count = base.sector_count();
        if sector_size == 0 {
            return Err(StorageError::InvalidImage(
                "El tamaño de sector del disco base debe ser mayor que cero".to_string(),
            ));
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .open(path)?;

        let mut header = [0u8; COW_HEADER_SIZE as usize];
        file.read_exact(&mut header)?;

        if &header[0..8] != COW_MAGIC {
            return Err(StorageError::InvalidImage(
                "Magic de overlay COW inválido".to_string(),
            ));
        }
        let version = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        if version != COW_VERSION {
            return Err(StorageError::InvalidImage(format!(
                "Versión de overlay COW no soportada: {}",
                version
            )));
        }
        let ov_sector_size = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        let ov_sector_count = u64::from_le_bytes([
            header[16], header[17], header[18], header[19], header[20], header[21], header[22], header[23],
        ]);
        let bitmap_offset = u64::from_le_bytes([
            header[24], header[25], header[26], header[27], header[28], header[29], header[30], header[31],
        ]);
        let data_offset = u64::from_le_bytes([
            header[32], header[33], header[34], header[35], header[36], header[37], header[38], header[39],
        ]);

        if ov_sector_size != sector_size || ov_sector_count != sector_count {
            return Err(StorageError::InvalidImage(format!(
                "Geometría del overlay ({}/{}) no coincide con el disco base ({}/{})",
                ov_sector_count, ov_sector_size, sector_count, sector_size
            )));
        }

        let expected_bitmap_len = bitmap_len_bytes(sector_count);
        let min_data_offset = bitmap_offset
            .checked_add(expected_bitmap_len as u64)
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento en metadatos de overlay".to_string()))?;
        if data_offset < min_data_offset {
            return Err(StorageError::InvalidImage(
                "Metadatos de overlay COW inválidos (data_offset)".to_string(),
            ));
        }

        let mut bitmap = vec![0u8; expected_bitmap_len];
        file.seek(SeekFrom::Start(bitmap_offset))?;
        file.read_exact(&mut bitmap)?;

        Ok(Self {
            base,
            file,
            sector_size,
            sector_count,
            bitmap_offset,
            data_offset,
            bitmap,
            read_only,
        })
    }

    /// Devuelve una referencia al disco base subyacente.
    pub fn base(&self) -> &B {
        &self.base
    }

    /// Devuelve una referencia mutable al disco base subyacente.
    /// Nota: escribir en la base puede romper la coherencia lógica del overlay.
    pub fn base_mut(&mut self) -> &mut B {
        &mut self.base
    }

    fn validate_range(&self, lba: u64, count: u32, buf_len: usize) -> Result<usize, StorageError> {
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
        Ok(expected)
    }

    #[inline]
    fn is_sector_present(&self, lba: u64) -> bool {
        let idx = lba as usize / 8;
        let bit = (lba as usize % 8) as u8;
        idx < self.bitmap.len() && (self.bitmap[idx] & (1 << bit)) != 0
    }

    #[inline]
    fn set_sector_present(&mut self, lba: u64) {
        let idx = lba as usize / 8;
        let bit = (lba as usize % 8) as u8;
        if idx < self.bitmap.len() {
            self.bitmap[idx] |= 1 << bit;
        }
    }

    fn data_offset_for(&self, lba: u64) -> Result<u64, StorageError> {
        lba.checked_mul(self.sector_size as u64)
            .and_then(|v| self.data_offset.checked_add(v))
            .ok_or_else(|| StorageError::InvalidImage("Desbordamiento de desplazamiento en overlay".to_string()))
    }

    fn persist_bitmap_byte(&mut self, lba: u64) -> Result<(), StorageError> {
        let idx = lba as usize / 8;
        if idx >= self.bitmap.len() {
            return Ok(());
        }
        let off = self.bitmap_offset + idx as u64;
        let byte = [self.bitmap[idx]];
        self.file.seek(SeekFrom::Start(off))?;
        self.file.write_all(&byte)?;
        Ok(())
    }
}

impl<B: BlockDevice> BlockDevice for CowOverlayDisk<B> {
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
        let _ = self.validate_range(lba, count, buf.len())?;
        let sector_size = self.sector_size as usize;

        // Segmenta la operación para mezclar lecturas desde overlay y base.
        let mut i = 0u32;
        while i < count {
            let cur_lba = lba + i as u64;
            let from_overlay = self.is_sector_present(cur_lba);
            let mut j = i + 1;
            while j < count && self.is_sector_present(lba + j as u64) == from_overlay {
                j += 1;
            }
            let seg_count = j - i;
            let start = i as usize * sector_size;
            let end = j as usize * sector_size;
            let seg_buf = &mut buf[start..end];

            if from_overlay {
                let off = self.data_offset_for(cur_lba)?;
                self.file.seek(SeekFrom::Start(off))?;
                self.file.read_exact(seg_buf)?;
            } else {
                self.base.read_sectors(cur_lba, seg_count, seg_buf)?;
            }
            i = j;
        }
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, count: u32, buf: &[u8]) -> Result<(), StorageError> {
        if self.read_only {
            return Err(StorageError::ReadOnly);
        }
        let _ = self.validate_range(lba, count, buf.len())?;

        let off = self.data_offset_for(lba)?;
        self.file.seek(SeekFrom::Start(off))?;
        self.file.write_all(buf)?;

        // Actualizar bitmap (por byte) para sectores recién escritos.
        let mut k = 0u32;
        while k < count {
            let cur = lba + k as u64;
            if !self.is_sector_present(cur) {
                self.set_sector_present(cur);
                self.persist_bitmap_byte(cur)?;
            } else {
                // ya presente; asegurar bit en memoria por consistencia (no-op efectivo)
                self.set_sector_present(cur);
            }
            k += 1;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StorageError> {
        self.file.sync_all()?;
        Ok(())
    }
}

fn bitmap_len_bytes(sector_count: u64) -> usize {
    ((sector_count + 7) / 8) as usize
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    let rem = value % align;
    if rem == 0 { value } else { value + (align - rem) }
}
