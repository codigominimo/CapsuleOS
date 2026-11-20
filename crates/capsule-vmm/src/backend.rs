use capsule_core::types::GuestAddress;
use capsule_cpu::Registers;

/// Acceso uniforme a registros para backends de vCPU.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuRegsSnapshot(pub Registers);

impl std::ops::Deref for CpuRegsSnapshot {
    type Target = Registers;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl std::ops::DerefMut for CpuRegsSnapshot {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

pub fn write_io_result_to_regs(regs: &mut Registers, size: u8, val: u32) {
    match size {
        1 => regs.rax = (regs.rax & !0xFF) | (val as u64 & 0xFF),
        2 => regs.rax = (regs.rax & !0xFFFF) | (val as u64 & 0xFFFF),
        4 => regs.set_eax(val),
        _ => {}
    }
}

#[allow(dead_code)]
pub fn _guest_addr(_a: GuestAddress) {}
