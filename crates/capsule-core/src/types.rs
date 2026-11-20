//! Tipos fundamentales para CapsuleOS.

use std::fmt;

/// Dirección física dentro del espacio de memoria del guest (GPA - Guest Physical Address).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct GuestAddress(pub u64);

impl GuestAddress {
    #[inline]
    pub const fn new(addr: u64) -> Self {
        GuestAddress(addr)
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn offset(self, delta: u64) -> Self {
        GuestAddress(self.0 + delta)
    }
}

impl fmt::Debug for GuestAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GPA(0x{:016X})", self.0)
    }
}

impl fmt::Display for GuestAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:016X}", self.0)
    }
}

/// Dirección virtual o física en el espacio del host (HVA - Host Virtual Address).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct HostAddress(pub usize);

impl HostAddress {
    #[inline]
    pub const fn new(addr: usize) -> Self {
        HostAddress(addr)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }

    #[inline]
    pub fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    #[inline]
    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl fmt::Debug for HostAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HVA(0x{:016X})", self.0)
    }
}

impl fmt::Display for HostAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:016X}", self.0)
    }
}

/// Unidades de memoria para formateo amigable.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ByteSize(pub u64);

impl ByteSize {
    pub const fn from_bytes(bytes: u64) -> Self {
        ByteSize(bytes)
    }

    pub const fn from_mib(mib: u64) -> Self {
        ByteSize(mib * 1024 * 1024)
    }

    pub const fn from_gib(gib: u64) -> Self {
        ByteSize(gib * 1024 * 1024 * 1024)
    }

    pub fn as_bytes(&self) -> u64 {
        self.0
    }

    pub fn as_kib_f64(&self) -> f64 {
        self.0 as f64 / 1024.0
    }

    pub fn as_mib_f64(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0)
    }

    pub fn as_gib_f64(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 >= 1024 * 1024 * 1024 {
            write!(f, "{:.2} GB", self.as_gib_f64())
        } else if self.0 >= 1024 * 1024 {
            write!(f, "{:.1} MB", self.as_mib_f64())
        } else if self.0 >= 1024 {
            write!(f, "{:.1} KB", self.as_kib_f64())
        } else {
            write!(f, "{} B", self.0)
        }
    }
}
