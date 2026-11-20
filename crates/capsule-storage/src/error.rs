use std::fmt;
use std::io;

#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    OutOfBounds { lba: u64, count: u32, sector_count: u64 },
    InvalidBufferSize { expected: usize, actual: usize },
    ReadOnly,
    InvalidImage(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io(err) => write!(f, "Error de E/S de almacenamiento: {}", err),
            StorageError::OutOfBounds { lba, count, sector_count } => write!(
                f,
                "Acceso fuera de rango: LBA {} + {} sectores (total {})",
                lba, count, sector_count
            ),
            StorageError::InvalidBufferSize { expected, actual } => write!(
                f,
                "Tamaño de búfer inválido: esperado {} bytes, recibido {}",
                expected, actual
            ),
            StorageError::ReadOnly => write!(f, "El dispositivo de bloque es de solo lectura"),
            StorageError::InvalidImage(msg) => write!(f, "Imagen de disco inválida: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        StorageError::Io(value)
    }
}
