use crate::error::StorageError;

/// Abstracción de un dispositivo de almacenamiento de bloques (disco virtual).
pub trait BlockDevice: Send {
    /// Tamaño de un sector en bytes (típicamente 512).
    fn sector_size(&self) -> u32;

    /// Número total de sectores direccionables.
    fn sector_count(&self) -> u64;

    /// Indica si el dispositivo no permite escrituras.
    fn is_read_only(&self) -> bool;

    /// Lee `count` sectores comenzando en `lba` dentro de `buf`.
    /// `buf.len()` debe ser exactamente `count * sector_size()`.
    fn read_sectors(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), StorageError>;

    /// Escribe `count` sectores comenzando en `lba` desde `buf`.
    /// `buf.len()` debe ser exactamente `count * sector_size()`.
    fn write_sectors(&mut self, lba: u64, count: u32, buf: &[u8]) -> Result<(), StorageError>;

    /// Persiste cambios pendientes en el medio subyacente, si aplica.
    fn flush(&mut self) -> Result<(), StorageError> {
        Ok(())
    }

    /// Tamaño total del dispositivo en bytes.
    fn capacity_bytes(&self) -> u64 {
        self.sector_count() * self.sector_size() as u64
    }
}
