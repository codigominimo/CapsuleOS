use capsule_bundle::{CapsuleBundle, CapsuleManifest, PayloadCompression};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bundle_roundtrip_file() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("capsule_bundle_{nanos}.cap"));
    let b = CapsuleBundle {
        manifest: CapsuleManifest {
            name: "ubuntu".to_string(),
            guest_os: "linux".to_string(),
            backend: "software".to_string(),
            memory_mib: 512,
            vcpus: 2,
        },
        payload: b"payload-data".to_vec(),
        compression: PayloadCompression::None,
    };
    b.write_to_file(&path).unwrap();
    let b2 = CapsuleBundle::read_from_file(&path).unwrap();
    assert_eq!(b, b2);
    let _ = std::fs::remove_file(path);
}

#[test]
fn bundle_append_to_exe_roundtrip() {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir();
    let fake_exe = dir.join(format!("capsule_fake_{nanos}.exe"));
    let out_exe = dir.join(format!("capsule_out_{nanos}.exe"));
    std::fs::write(&fake_exe, b"MZFAKEEXE").unwrap();
    let b = CapsuleBundle {
        manifest: CapsuleManifest { name: "n".into(), guest_os: "linux".into(), backend: "software".into(), memory_mib: 64, vcpus: 1 },
        payload: b"hello".to_vec(),
        compression: PayloadCompression::None,
    };
    CapsuleBundle::append_to_exe(&fake_exe, &out_exe, &b).unwrap();
    let found = CapsuleBundle::read_from_appended_exe(&out_exe).unwrap().unwrap();
    assert_eq!(found, b);
    let _ = std::fs::remove_file(fake_exe);
    let _ = std::fs::remove_file(out_exe);
}
