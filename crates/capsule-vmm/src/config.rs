use capsule_core::types::ByteSize;

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub name: String,
    pub vcpu_count: usize,
    pub memory_size: ByteSize,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            name: "CapsuleGuest".to_string(),
            vcpu_count: 1,
            memory_size: ByteSize::from_mib(128),
        }
    }
}
