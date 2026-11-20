use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkError {
    InvalidFrame(String),
    Io(String),
    Unsupported(String),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::InvalidFrame(msg) => write!(f, "Trama de red inválida: {msg}"),
            NetworkError::Io(msg) => write!(f, "Error de E/S de red: {msg}"),
            NetworkError::Unsupported(msg) => write!(f, "Operación de red no soportada: {msg}"),
        }
    }
}

impl std::error::Error for NetworkError {}
