use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"CAPSULE\0";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PayloadCompression {
    None = 0,
}

impl PayloadCompression {
    fn from_u32(v: u32) -> Result<Self, String> {
        match v {
            0 => Ok(Self::None),
            _ => Err(format!("unsupported compression {v}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsuleManifest {
    pub name: String,
    pub guest_os: String,
    pub backend: String,
    pub memory_mib: u32,
    pub vcpus: u32,
}

impl CapsuleManifest {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_kv(&mut out, "name", &self.name);
        push_kv(&mut out, "guest_os", &self.guest_os);
        push_kv(&mut out, "backend", &self.backend);
        push_kv(&mut out, "memory_mib", &self.memory_mib.to_string());
        push_kv(&mut out, "vcpus", &self.vcpus.to_string());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let s = std::str::from_utf8(bytes).map_err(|e| format!("manifest utf8: {e}"))?;
        let mut name = None;
        let mut guest_os = None;
        let mut backend = None;
        let mut memory_mib = None;
        let mut vcpus = None;
        for line in s.lines() {
            if line.trim().is_empty() { continue; }
            let (k, v) = line.split_once('=').ok_or_else(|| format!("manifest line: {line}"))?;
            match k {
                "name" => name = Some(v.to_string()),
                "guest_os" => guest_os = Some(v.to_string()),
                "backend" => backend = Some(v.to_string()),
                "memory_mib" => memory_mib = Some(v.parse::<u32>().map_err(|e| e.to_string())?),
                "vcpus" => vcpus = Some(v.parse::<u32>().map_err(|e| e.to_string())?),
                _ => {}
            }
        }
        Ok(Self {
            name: name.ok_or("manifest missing name")?,
            guest_os: guest_os.ok_or("manifest missing guest_os")?,
            backend: backend.ok_or("manifest missing backend")?,
            memory_mib: memory_mib.ok_or("manifest missing memory_mib")?,
            vcpus: vcpus.ok_or("manifest missing vcpus")?,
        })
    }
}

fn push_kv(out: &mut Vec<u8>, k: &str, v: &str) {
    out.extend_from_slice(k.as_bytes());
    out.push(b'=');
    out.extend_from_slice(v.as_bytes());
    out.push(b'\n');
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsuleBundle {
    pub manifest: CapsuleManifest,
    pub payload: Vec<u8>,
    pub compression: PayloadCompression,
}

impl CapsuleBundle {
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let manifest_bytes = self.manifest.to_bytes();
        let payload_bytes = match self.compression {
            PayloadCompression::None => self.payload.clone(),
        };
        let mut f = File::create(path).map_err(|e| e.to_string())?;
        f.write_all(MAGIC).map_err(|e| e.to_string())?;
        f.write_all(&VERSION.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(self.compression as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(manifest_bytes.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(payload_bytes.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(self.payload.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?; // uncompressed len
        f.write_all(&manifest_bytes).map_err(|e| e.to_string())?;
        f.write_all(&payload_bytes).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn read_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Self::from_bytes(&buf)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 8 + 4 + 4 + 8 + 8 + 8 { return Err("bundle too small".into()); }
        if &buf[0..8] != MAGIC { return Err("bad magic".into()); }
        let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        if version != VERSION { return Err(format!("unsupported version {version}")); }
        let compression = PayloadCompression::from_u32(u32::from_le_bytes(buf[12..16].try_into().unwrap()))?;
        let manifest_len = u64::from_le_bytes(buf[16..24].try_into().unwrap()) as usize;
        let payload_len = u64::from_le_bytes(buf[24..32].try_into().unwrap()) as usize;
        let _uncompressed_len = u64::from_le_bytes(buf[32..40].try_into().unwrap()) as usize;
        let off = 40;
        if buf.len() < off + manifest_len + payload_len { return Err("bundle truncated".into()); }
        let manifest = CapsuleManifest::from_bytes(&buf[off..off + manifest_len])?;
        let payload_stored = &buf[off + manifest_len..off + manifest_len + payload_len];
        let payload = match compression {
            PayloadCompression::None => payload_stored.to_vec(),
        };
        Ok(Self { manifest, payload, compression })
    }
}
