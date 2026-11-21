use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

fn decode_payload(compression: PayloadCompression, payload_stored: &[u8], uncompressed_len: usize) -> Result<Vec<u8>, String> {
    let payload = match compression {
        PayloadCompression::None => payload_stored.to_vec(),
        PayloadCompression::Deflate => {
            let mut dec = flate2::read::DeflateDecoder::new(payload_stored);
            let mut out = Vec::new();
            dec.read_to_end(&mut out).map_err(|e| format!("deflate decode: {e}"))?;
            out
        }
    };
    if payload.len() != uncompressed_len {
        return Err(format!("payload length mismatch: header {uncompressed_len} actual {}", payload.len()));
    }
    Ok(payload)
}

pub const MAGIC: &[u8; 8] = b"CAPSULE\0";
pub const VERSION: u32 = 3;
pub const VERSION_V2: u32 = 2;
pub const VERSION_V1: u32 = 1;
pub const EXE_FOOTER_MAGIC: &[u8; 8] = b"CAPSLEXE";
pub const EXE_FOOTER_LEN: usize = 24; // magic(8) + bundle_len(u64) + version(u32) + reserved(u32)
pub const HEADER_LEN_V2: usize = 56; // magic8 + ver4 + comp4 + manifest_len8 + payload_stored8 + payload_uncompressed8 + manifest_crc4 + payload_crc4 + header_flags4 + reserved4
pub const HEADER_LEN: usize = 160; // v3: v2 fields + sha256 manifest(32) + sha256 payload(32) + hmac(32) + flags(4) + reserved(4)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PayloadCompression {
    None = 0,
    Deflate = 1,
}

impl PayloadCompression {
    fn from_u32(v: u32) -> Result<Self, String> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Deflate),
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
    pub payload_kind: String, // raw-disk|linux-kernel|boot-sector|opaque
    pub kernel_cmdline: Option<String>,
    pub initrd_bytes: Option<u64>,
    pub disk_sector_size: Option<u32>,
}

impl CapsuleManifest {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_kv(&mut out, "name", &self.name);
        push_kv(&mut out, "guest_os", &self.guest_os);
        push_kv(&mut out, "backend", &self.backend);
        push_kv(&mut out, "memory_mib", &self.memory_mib.to_string());
        push_kv(&mut out, "vcpus", &self.vcpus.to_string());
        push_kv(&mut out, "payload_kind", &self.payload_kind);
        if let Some(v) = &self.kernel_cmdline { push_kv(&mut out, "kernel_cmdline", v); }
        if let Some(v) = self.initrd_bytes { push_kv(&mut out, "initrd_bytes", &v.to_string()); }
        if let Some(v) = self.disk_sector_size { push_kv(&mut out, "disk_sector_size", &v.to_string()); }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let s = std::str::from_utf8(bytes).map_err(|e| format!("manifest utf8: {e}"))?;
        let mut name = None;
        let mut guest_os = None;
        let mut backend = None;
        let mut memory_mib = None;
        let mut vcpus = None;
        let mut payload_kind = None;
        let mut kernel_cmdline = None;
        let mut initrd_bytes = None;
        let mut disk_sector_size = None;
        for line in s.lines() {
            if line.trim().is_empty() { continue; }
            let (k, v) = line.split_once('=').ok_or_else(|| format!("manifest line: {line}"))?;
            match k {
                "name" => name = Some(v.to_string()),
                "guest_os" => guest_os = Some(v.to_string()),
                "backend" => backend = Some(v.to_string()),
                "memory_mib" => memory_mib = Some(v.parse::<u32>().map_err(|e| e.to_string())?),
                "vcpus" => vcpus = Some(v.parse::<u32>().map_err(|e| e.to_string())?),
                "payload_kind" => payload_kind = Some(v.to_string()),
                "kernel_cmdline" => kernel_cmdline = Some(v.to_string()),
                "initrd_bytes" => initrd_bytes = Some(v.parse::<u64>().map_err(|e| e.to_string())?),
                "disk_sector_size" => disk_sector_size = Some(v.parse::<u32>().map_err(|e| e.to_string())?),
                _ => {}
            }
        }
        Ok(Self {
            name: name.ok_or("manifest missing name")?,
            guest_os: guest_os.ok_or("manifest missing guest_os")?,
            backend: backend.ok_or("manifest missing backend")?,
            memory_mib: memory_mib.ok_or("manifest missing memory_mib")?,
            vcpus: vcpus.ok_or("manifest missing vcpus")?,
            payload_kind: payload_kind.unwrap_or_else(|| "opaque".to_string()),
            kernel_cmdline,
            initrd_bytes,
            disk_sector_size,
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
    pub hmac_key: Option<Vec<u8>>, // when present, v3 writes/verifies HMAC-SHA256 over header+manifest+stored payload
}

impl CapsuleBundle {
    pub fn with_hmac_key(mut self, key: Vec<u8>) -> Self { self.hmac_key = Some(key); self }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        self.write_to_bytes(&mut bytes)?;
        use std::io::Write as _;
        f.write_all(&bytes).map_err(|e| e.to_string())
    }

    pub fn read_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Self::from_bytes(&buf)
    }

    pub fn read_from_file_with_key(path: impl AsRef<Path>, key: &[u8]) -> Result<Self, String> {
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Self::from_bytes_with_key(&buf, Some(key))
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> { Self::from_bytes_with_key(buf, None) }

    pub fn from_bytes_with_key(buf: &[u8], hmac_key: Option<&[u8]>) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<Sha256>;
        if buf.len() < 40 { return Err("bundle too small".into()); }
        if &buf[0..8] != MAGIC { return Err("bad magic".into()); }
        let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let compression = PayloadCompression::from_u32(u32::from_le_bytes(buf[12..16].try_into().unwrap()))?;
        let manifest_len = u64::from_le_bytes(buf[16..24].try_into().unwrap()) as usize;
        let payload_len = u64::from_le_bytes(buf[24..32].try_into().unwrap()) as usize;
        let uncompressed_len = u64::from_le_bytes(buf[32..40].try_into().unwrap()) as usize;

        match version {
            VERSION_V1 => {
                let off = 40;
                if buf.len() < off + manifest_len + payload_len { return Err("bundle truncated".into()); }
                let manifest = CapsuleManifest::from_bytes(&buf[off..off + manifest_len])?;
                let payload_stored = &buf[off + manifest_len..off + manifest_len + payload_len];
                let payload = match compression { PayloadCompression::None => payload_stored.to_vec(), _ => return Err("v1 only supports compression none".into()) };
                Ok(Self { manifest, payload, compression, hmac_key: hmac_key.map(|k| k.to_vec()) })
            }
            VERSION_V2 => {
                if buf.len() < HEADER_LEN_V2 { return Err("bundle too small".into()); }
                let manifest_crc = u32::from_le_bytes(buf[40..44].try_into().unwrap());
                let payload_crc = u32::from_le_bytes(buf[44..48].try_into().unwrap());
                let off = HEADER_LEN_V2;
                if buf.len() < off + manifest_len + payload_len { return Err("bundle truncated".into()); }
                let manifest_bytes = &buf[off..off + manifest_len];
                let payload_stored = &buf[off + manifest_len..off + manifest_len + payload_len];
                let actual_manifest_crc = crc32fast::hash(manifest_bytes);
                if actual_manifest_crc != manifest_crc { return Err(format!("manifest crc mismatch: expected {manifest_crc:08x} got {actual_manifest_crc:08x}")); }
                let payload = decode_payload(compression, payload_stored, uncompressed_len)?;
                let actual_payload_crc = crc32fast::hash(&payload);
                if actual_payload_crc != payload_crc { return Err(format!("payload crc mismatch: expected {payload_crc:08x} got {actual_payload_crc:08x}")); }
                let manifest = CapsuleManifest::from_bytes(manifest_bytes)?;
                Ok(Self { manifest, payload, compression, hmac_key: hmac_key.map(|k| k.to_vec()) })
            }
            VERSION => {
                if buf.len() < HEADER_LEN { return Err("bundle too small".into()); }
                let manifest_crc = u32::from_le_bytes(buf[40..44].try_into().unwrap());
                let payload_crc = u32::from_le_bytes(buf[44..48].try_into().unwrap());
                let _header_flags = u32::from_le_bytes(buf[48..52].try_into().unwrap());
                let manifest_sha: [u8;32] = buf[56..88].try_into().unwrap();
                let payload_sha: [u8;32] = buf[88..120].try_into().unwrap();
                let hmac_expected: [u8;32] = buf[120..152].try_into().unwrap();
                let off = HEADER_LEN;
                if buf.len() < off + manifest_len + payload_len { return Err("bundle truncated".into()); }
                let manifest_bytes = &buf[off..off + manifest_len];
                let payload_stored = &buf[off + manifest_len..off + manifest_len + payload_len];
                if crc32fast::hash(manifest_bytes) != manifest_crc { return Err("manifest crc mismatch".into()); }
                let actual_manifest_sha: [u8;32] = Sha256::digest(manifest_bytes).into();
                if actual_manifest_sha != manifest_sha { return Err("manifest sha256 mismatch".into()); }
                let payload = decode_payload(compression, payload_stored, uncompressed_len)?;
                if crc32fast::hash(&payload) != payload_crc { return Err("payload crc mismatch".into()); }
                let actual_payload_sha: [u8;32] = Sha256::digest(&payload).into();
                if actual_payload_sha != payload_sha { return Err("payload sha256 mismatch".into()); }
                let key = hmac_key.unwrap_or(&[]);
                let mut mac = HmacSha256::new_from_slice(key).map_err(|e| format!("hmac key: {e}"))?;
                mac.update(&buf[0..120]);
                mac.update(&buf[152..HEADER_LEN]);
                mac.update(manifest_bytes);
                mac.update(payload_stored);
                let actual_hmac: [u8;32] = mac.finalize().into_bytes().into();
                if actual_hmac != hmac_expected { return Err("hmac mismatch (wrong key or tampered bundle)".into()); }
                let manifest = CapsuleManifest::from_bytes(manifest_bytes)?;
                Ok(Self { manifest, payload, compression, hmac_key: hmac_key.map(|k| k.to_vec()) })
            }
            other => Err(format!("unsupported version {other}")),
        }
    }
    pub fn append_to_exe(exe_path: impl AsRef<Path>, out_exe: impl AsRef<Path>, bundle: &Self) -> Result<(), String> {
        let mut exe = std::fs::read(&exe_path).map_err(|e| e.to_string())?;
        let mut bundle_bytes = Vec::new();
        bundle.write_to_bytes(&mut bundle_bytes)?;
        let bundle_len = bundle_bytes.len() as u64;
        exe.extend_from_slice(&bundle_bytes);
        exe.extend_from_slice(EXE_FOOTER_MAGIC);
        exe.extend_from_slice(&bundle_len.to_le_bytes());
        exe.extend_from_slice(&VERSION.to_le_bytes());
        exe.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(out_exe, exe).map_err(|e| e.to_string())
    }

        pub fn read_from_appended_exe(path: impl AsRef<Path>) -> Result<Option<Self>, String> {
        Self::read_from_appended_exe_with_key(path, None)
    }

    pub fn read_from_appended_exe_with_key(path: impl AsRef<Path>, hmac_key: Option<&[u8]>) -> Result<Option<Self>, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        if data.len() < EXE_FOOTER_LEN { return Ok(None); }
        let foot = &data[data.len() - EXE_FOOTER_LEN..];
        if &foot[0..8] != EXE_FOOTER_MAGIC { return Ok(None); }
        let bundle_len = u64::from_le_bytes(foot[8..16].try_into().unwrap()) as usize;
        let version = u32::from_le_bytes(foot[16..20].try_into().unwrap());
        if !matches!(version, VERSION | VERSION_V2 | VERSION_V1) { return Err(format!("unsupported appended bundle version {version}")); }
        if data.len() < EXE_FOOTER_LEN + bundle_len { return Err("appended bundle truncated".into()); }
        let start = data.len() - EXE_FOOTER_LEN - bundle_len;
        let bundle = Self::from_bytes_with_key(&data[start..start + bundle_len], hmac_key)?;
        Ok(Some(bundle))
    }
pub fn write_to_bytes<W: Write>(&self, mut w: W) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<Sha256>;
        let manifest_bytes = self.manifest.to_bytes();
        let payload_stored = match self.compression {
            PayloadCompression::None => self.payload.clone(),
            PayloadCompression::Deflate => {
                let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(&self.payload).map_err(|e| format!("deflate encode: {e}"))?;
                enc.finish().map_err(|e| format!("deflate finish: {e}"))?
            }
        };
        let manifest_crc = crc32fast::hash(&manifest_bytes);
        let payload_crc = crc32fast::hash(&self.payload);
        let manifest_sha: [u8;32] = Sha256::digest(&manifest_bytes).into();
        let payload_sha: [u8;32] = Sha256::digest(&self.payload).into();
        let key = self.hmac_key.as_deref().unwrap_or(&[]);
        let mut header = [0u8; HEADER_LEN];
        header[0..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&VERSION.to_le_bytes());
        header[12..16].copy_from_slice(&(self.compression as u32).to_le_bytes());
        header[16..24].copy_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
        header[24..32].copy_from_slice(&(payload_stored.len() as u64).to_le_bytes());
        header[32..40].copy_from_slice(&(self.payload.len() as u64).to_le_bytes());
        header[40..44].copy_from_slice(&manifest_crc.to_le_bytes());
        header[44..48].copy_from_slice(&payload_crc.to_le_bytes());
        header[48..52].copy_from_slice(&0u32.to_le_bytes());
        header[52..56].copy_from_slice(&0u32.to_le_bytes());
        header[56..88].copy_from_slice(&manifest_sha);
        header[88..120].copy_from_slice(&payload_sha);
        header[152..156].copy_from_slice(&0u32.to_le_bytes());
        header[156..160].copy_from_slice(&0u32.to_le_bytes());
        let mut mac = HmacSha256::new_from_slice(key).map_err(|e| format!("hmac key: {e}"))?;
        mac.update(&header[0..120]);
        mac.update(&header[152..HEADER_LEN]);
        mac.update(&manifest_bytes);
        mac.update(&payload_stored);
        let hmac_bytes: [u8;32] = mac.finalize().into_bytes().into();
        header[120..152].copy_from_slice(&hmac_bytes);
        w.write_all(&header).map_err(|e| e.to_string())?;
        w.write_all(&manifest_bytes).map_err(|e| e.to_string())?;
        w.write_all(&payload_stored).map_err(|e| e.to_string())?;
        Ok(())
    }
}