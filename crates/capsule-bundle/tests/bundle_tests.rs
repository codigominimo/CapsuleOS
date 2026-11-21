use capsule_bundle::{CapsuleBundle, CapsuleManifest, PayloadCompression};
use std::time::{SystemTime, UNIX_EPOCH};

fn manifest(name: &str) -> CapsuleManifest {
    CapsuleManifest {
        name: name.to_string(),
        guest_os: "linux".to_string(),
        backend: "software".to_string(),
        memory_mib: 512,
        vcpus: 2,
        payload_kind: "opaque".to_string(),
        kernel_cmdline: None,
        initrd_bytes: None,
        disk_sector_size: None,
    }
}

#[test]
fn bundle_roundtrip_file() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("capsule_bundle_{nanos}.cap"));
    let b = CapsuleBundle { manifest: manifest("ubuntu"), payload: b"payload-data".to_vec(), compression: PayloadCompression::None, hmac_key: None };
    b.write_to_file(&path).unwrap();
    let b2 = CapsuleBundle::read_from_file(&path).unwrap();
    assert_eq!(b, b2);
    let _ = std::fs::remove_file(path);
}

#[test]
fn bundle_roundtrip_deflate_and_crc() {
    let payload = vec![42u8; 4096];
    let b = CapsuleBundle { manifest: manifest("compressed"), payload: payload.clone(), compression: PayloadCompression::Deflate, hmac_key: None };
    let mut bytes = Vec::new();
    b.write_to_bytes(&mut bytes).unwrap();
    // stored payload should be smaller for repetitive data
    assert!(bytes.len() < payload.len() + 200);
    let b2 = CapsuleBundle::from_bytes(&bytes).unwrap();
    assert_eq!(b2.payload, payload);
    assert_eq!(b2.compression, PayloadCompression::Deflate);
    // corrupt payload crc
    let mut bad = bytes.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(CapsuleBundle::from_bytes(&bad).is_err());
}

#[test]
fn bundle_append_to_exe_roundtrip() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir();
    let fake_exe = dir.join(format!("capsule_fake_{nanos}.exe"));
    let out_exe = dir.join(format!("capsule_out_{nanos}.exe"));
    std::fs::write(&fake_exe, b"MZFAKEEXE").unwrap();
    let b = CapsuleBundle { manifest: manifest("n"), payload: b"hello".to_vec(), compression: PayloadCompression::Deflate, hmac_key: Some(b"test-key".to_vec()) };
    CapsuleBundle::append_to_exe(&fake_exe, &out_exe, &b).unwrap();
    let found = CapsuleBundle::read_from_appended_exe_with_key(&out_exe, Some(b"test-key")).unwrap().unwrap();
    assert_eq!(found.payload, b.payload);
    let _ = std::fs::remove_file(fake_exe);
    let _ = std::fs::remove_file(out_exe);
}
