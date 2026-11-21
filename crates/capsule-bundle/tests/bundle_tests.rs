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
