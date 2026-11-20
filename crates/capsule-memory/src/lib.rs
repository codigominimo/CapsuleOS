//! # capsule-memory
//!
//! Gestión de memoria física del guest (GPA) y mapeo en el espacio de direcciones del host (HVA).

use capsule_core::types::{GuestAddress, HostAddress};
use std::fmt;
use std::ptr::null_mut;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};

#[derive(Debug)]
pub enum MemoryError {
    AllocationFailed(usize),
    OutOfBounds { address: GuestAddress, size: usize },
    InvalidAccess(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::AllocationFailed(size) => {
                write!(f, "Fallo al reservar {} bytes en memoria host", size)
            }
            MemoryError::OutOfBounds { address, size } => {
                write!(
                    f,
                    "Acceso fuera de límites en GPA: {} con tamaño {}",
                    address, size
                )
            }
            MemoryError::InvalidAccess(msg) => write!(f, "Acceso a memoria no válido: {}", msg),
        }
    }
}

impl std::error::Error for MemoryError {}

/// Memoria física contigua del guest (Guest Physical Memory).
pub struct GuestMemory {
    base_hva: *mut u8,
    size: usize,
}

// GuestMemory es segura de transferir entre hilos
unsafe impl Send for GuestMemory {}
unsafe impl Sync for GuestMemory {}

impl GuestMemory {
    /// Reserva una región de memoria física para el guest con tamaño especificado en bytes.
    /// La memoria se alinea a los límites de página del host (4KB).
    pub fn new(size: usize) -> Result<Self, MemoryError> {
        if size == 0 {
            return Err(MemoryError::AllocationFailed(0));
        }

        unsafe {
            let ptr = VirtualAlloc(
                null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );

            if ptr.is_null() {
                return Err(MemoryError::AllocationFailed(size));
            }

            Ok(Self {
                base_hva: ptr as *mut u8,
                size,
            })
        }
    }

    /// Retorna el tamaño total de la memoria en bytes.
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Retorna la dirección virtual del host (HVA) de la base de memoria.
    #[inline]
    pub fn host_base_address(&self) -> HostAddress {
        HostAddress::new(self.base_hva as usize)
    }

    /// Retorna un puntero constante a la memoria del host.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.base_hva
    }

    /// Retorna un puntero mutable a la memoria del host.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.base_hva
    }

    /// Verifica si un rango `[gpa, gpa + len)` cae dentro del rango válido de memoria física.
    #[inline]
    pub fn is_valid_range(&self, gpa: GuestAddress, len: usize) -> bool {
        let start = gpa.raw() as usize;
        start.checked_add(len).map_or(false, |end| end <= self.size)
    }

    /// Lee un byte en la dirección `gpa`.
    pub fn read_u8(&self, gpa: GuestAddress) -> Result<u8, MemoryError> {
        let offset = gpa.raw() as usize;
        if offset >= self.size {
            return Err(MemoryError::OutOfBounds {
                address: gpa,
                size: 1,
            });
        }
        unsafe { Ok(*self.base_hva.add(offset)) }
    }

    /// Escribe un byte en la dirección `gpa`.
    pub fn write_u8(&mut self, gpa: GuestAddress, val: u8) -> Result<(), MemoryError> {
        let offset = gpa.raw() as usize;
        if offset >= self.size {
            return Err(MemoryError::OutOfBounds {
                address: gpa,
                size: 1,
            });
        }
        unsafe {
            *self.base_hva.add(offset) = val;
        }
        Ok(())
    }

    /// Lee un entero de 16 bits (little-endian) en la dirección `gpa`.
    pub fn read_u16(&self, gpa: GuestAddress) -> Result<u16, MemoryError> {
        let mut buf = [0u8; 2];
        self.read_bytes(gpa, &mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Lee un entero de 32 bits (little-endian) en la dirección `gpa`.
    pub fn read_u32(&self, gpa: GuestAddress) -> Result<u32, MemoryError> {
        let mut buf = [0u8; 4];
        self.read_bytes(gpa, &mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    /// Lee un entero de 64 bits (little-endian) en la dirección `gpa`.
    pub fn read_u64(&self, gpa: GuestAddress) -> Result<u64, MemoryError> {
        let mut buf = [0u8; 8];
        self.read_bytes(gpa, &mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Escribe un bloque de bytes a partir de la dirección `gpa`.
    pub fn write_bytes(&mut self, gpa: GuestAddress, data: &[u8]) -> Result<(), MemoryError> {
        if !self.is_valid_range(gpa, data.len()) {
            return Err(MemoryError::OutOfBounds {
                address: gpa,
                size: data.len(),
            });
        }
        let offset = gpa.raw() as usize;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.base_hva.add(offset), data.len());
        }
        Ok(())
    }

    /// Lee un bloque de bytes a partir de la dirección `gpa`.
    pub fn read_bytes(&self, gpa: GuestAddress, buf: &mut [u8]) -> Result<(), MemoryError> {
        if !self.is_valid_range(gpa, buf.len()) {
            return Err(MemoryError::OutOfBounds {
                address: gpa,
                size: buf.len(),
            });
        }
        let offset = gpa.raw() as usize;
        unsafe {
            std::ptr::copy_nonoverlapping(self.base_hva.add(offset), buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }
}

impl Drop for GuestMemory {
    fn drop(&mut self) {
        if !self.base_hva.is_null() {
            unsafe {
                VirtualFree(self.base_hva as *mut _, 0, MEM_RELEASE);
            }
        }
    }
}
