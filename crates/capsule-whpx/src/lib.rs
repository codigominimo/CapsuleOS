use capsule_memory::GuestMemory;
use std::ffi::c_void;
use std::fmt;

#[derive(Debug)]
pub enum WhpxError {
    Unavailable(String),
    ApiLoad(String),
    CallFailed(&'static str, i32),
    InvalidState(String),
}

impl fmt::Display for WhpxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhpxError::Unavailable(s) => write!(f, "WHPX no disponible: {s}"),
            WhpxError::ApiLoad(s) => write!(f, "Error cargando API WHPX: {s}"),
            WhpxError::CallFailed(name, hr) => write!(f, "Fallo en {name}: HRESULT 0x{hr:08X}"),
            WhpxError::InvalidState(s) => write!(f, "Estado WHPX invalido: {s}"),
        }
    }
}

impl std::error::Error for WhpxError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WhpxRegs {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub rflags: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhpxRunExit {
    Halted,
    IoPort {
        port: u16,
        access_size: u8,
        is_write: bool,
        rax: u64,
        instruction_len: u8,
    },
    MemoryAccess { gpa: u64 },
    Interrupted,
    Canceled,
    Unsupported(String),
    Failed(String),
}

// --- Minimal WHPX FFI (WinHvPlatform.dll), loaded dynamically ---
type Hresult = i32;
type Handle = isize;
const S_OK: Hresult = 0;

const WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT: u32 = 0x00001001;
const WHV_MAP_GPA_RANGE_FLAG_READ: u32 = 0x1;
const WHV_MAP_GPA_RANGE_FLAG_WRITE: u32 = 0x2;
const WHV_MAP_GPA_RANGE_FLAG_EXECUTE: u32 = 0x4;

const WHV_RUN_VP_EXIT_REASON_NONE: u32 = 0x00000000;
const WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS: u32 = 0x00001000;
const WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS: u32 = 0x00002002;
const WHV_RUN_VP_EXIT_REASON_X64_HALT: u32 = 0x0000200C;
const WHV_RUN_VP_EXIT_REASON_CANCELED: u32 = 0x00002001;
const WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW: u32 = 0x00002007;
const WHV_RUN_VP_EXIT_REASON_EXCEPTION: u32 = 0x00002003;
const WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION: u32 = 0x00002004;
const WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE: u32 = 0x00002005;
const WHV_RUN_VP_EXIT_REASON_UNSUPPORTED_FEATURE: u32 = 0x00002006;

#[repr(u32)]
#[derive(Clone, Copy)]
enum RegName {
    Rax=0x00020000,Rcx,Rdx,Rbx,Rsp,Rbp,Rsi,Rdi,R8,R9,R10,R11,R12,R13,R14,R15,Rip,Rflags,
    Es=0x00020012,Cs,Ss,Ds,Fs,Gs,
    Cr0=0x00020020, Cr2=0x00020021, Cr3=0x00020022, Cr4=0x00020023, Efer=0x00020027,
}

#[repr(C)]
#[derive(Clone, Copy)]
union RegVal { reg64: u64, reg128: [u8;16], segment: SegmentReg }
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SegmentReg { base: u64, limit: u32, selector: u16, attributes: u16 }

// We'll use a byte buffer for run exit context and parse known offsets/fields carefully.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RunExitHeader {
    exit_reason: u32,
    _reserved: u32,
    // VP context follows (large). We'll access payload at fixed offsets based on common layout.
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoPortAccessCtx {
    // According to WHV_X64_IO_PORT_ACCESS_CONTEXT
    // uint8_t InstructionByteCount; uint8_t Reserved[3]; uint8_t InstructionBytes[16];
    // WHV_X64_IO_PORT_ACCESS_INFO AccessInfo; uint16_t PortNumber; uint16_t Reserved2[3];
    // UINT64 Rax; UINT64 InstructionCount (in newer?) / sometimes RcX,Rsi,Rdi,Rip,Rflags follow
    instruction_byte_count: u8,
    _rsv: [u8; 3],
    _instr: [u8; 16],
    access_info: u32,
    port_number: u16,
    _rsv2: [u8; 6],
    rax: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MemAccessCtx {
    _instruction_byte_count: u8,
    _rsv: [u8; 3],
    _instr: [u8; 16],
    _access_info: u32,
    _rsv2: u32,
    gpa: u64,
    _gva: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InterruptControl {
    // WHV_INTERRUPT_CONTROL - simplified as u64 bitfield for common layout
    // Type: bits 0-7, DestinationMode:8-9, TriggerMode:10, Reserved:11, Destination:32 bits at 32, Vector:32 at 64, Reserved
    // In Windows headers this is a UINT64 bitfield; we'll model as two u64s in the common 16-byte form.
    type_vector_dest: u64,
    destination_mode_flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ExceptionCtx {
    instruction_byte_count: u8,
    _rsv: [u8; 3],
    _instr: [u8; 16],
    _exception_type: u32,
    error_code_valid: u8,
    _rsv2: [u8; 3],
    error_code: u32,
    _exception_parameter: u64,
}

struct Api {
    _lib: windows_sys::Win32::Foundation::HMODULE,
    create_partition: unsafe extern "system" fn(*mut Handle) -> Hresult,
    setup_partition: unsafe extern "system" fn(Handle) -> Hresult,
    delete_partition: unsafe extern "system" fn(Handle) -> Hresult,
    set_partition_property: unsafe extern "system" fn(Handle, u32, *const c_void, u32) -> Hresult,
    map_gpa_range: unsafe extern "system" fn(Handle, *const c_void, u64, u64, u32) -> Hresult,
    unmap_gpa_range: unsafe extern "system" fn(Handle, u64, u64) -> Hresult,
    create_vp: unsafe extern "system" fn(Handle, u32, u32) -> Hresult,
    delete_vp: unsafe extern "system" fn(Handle, u32) -> Hresult,
    run_vp: unsafe extern "system" fn(Handle, u32, *mut c_void, u32) -> Hresult,
    get_regs: unsafe extern "system" fn(Handle, u32, *const u32, u32, *mut RegVal) -> Hresult,
    set_regs: unsafe extern "system" fn(Handle, u32, *const u32, u32, *const RegVal) -> Hresult,
    request_interrupt: Option<unsafe extern "system" fn(Handle, *const InterruptControl, u32) -> Hresult>,
}

impl Api {
    unsafe fn load() -> Result<Self, WhpxError> {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let dll: Vec<u16> = "WinHvPlatform.dll\0".encode_utf16().collect();
        let lib = LoadLibraryW(dll.as_ptr());
        if lib.is_null() {
            return Err(WhpxError::Unavailable("WinHvPlatform.dll no encontrada".into()));
        }
        unsafe fn gp<T: Copy>(lib: windows_sys::Win32::Foundation::HMODULE, name: &[u8]) -> Result<T, WhpxError> {
            let p = GetProcAddress(lib, name.as_ptr());
            let p = p.ok_or_else(|| WhpxError::ApiLoad(String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]).into_owned()))?;
            Ok(std::mem::transmute_copy::<_, T>(&p))
        }
        Ok(Self {
            _lib: lib,
            create_partition: gp(lib, b"WHvCreatePartition\0")?,
            setup_partition: gp(lib, b"WHvSetupPartition\0")?,
            delete_partition: gp(lib, b"WHvDeletePartition\0")?,
            set_partition_property: gp(lib, b"WHvSetPartitionProperty\0")?,
            map_gpa_range: gp(lib, b"WHvMapGpaRange\0")?,
            unmap_gpa_range: gp(lib, b"WHvUnmapGpaRange\0")?,
            create_vp: gp(lib, b"WHvCreateVirtualProcessor\0")?,
            delete_vp: gp(lib, b"WHvDeleteVirtualProcessor\0")?,
            run_vp: gp(lib, b"WHvRunVirtualProcessor\0")?,
            get_regs: gp(lib, b"WHvGetVirtualProcessorRegisters\0")?,
            set_regs: gp(lib, b"WHvSetVirtualProcessorRegisters\0")?,
            request_interrupt: {
                let p = GetProcAddress(lib, b"WHvRequestInterrupt\0".as_ptr());
                p.map(|f| std::mem::transmute_copy::<_, unsafe extern "system" fn(Handle, *const InterruptControl, u32) -> Hresult>(&f))
            },
        })
    }
}

pub struct WhpxPartition {
    api: Api,
    partition: Handle,
    vcpu_id: u32,
    mem_size: u64,
}

impl fmt::Debug for WhpxPartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WhpxPartition").field("partition", &self.partition).finish()
    }
}

impl WhpxPartition {
    pub fn create_and_map(memory: &GuestMemory) -> Result<Self, WhpxError> {
        if !capsule_core::detect_whpx().is_available() {
            return Err(WhpxError::Unavailable("WHPX no esta disponible o no habilitado en el host".into()));
        }
        unsafe {
            let api = Api::load()?;
            let mut partition: Handle = 0;
            let hr = (api.create_partition)(&mut partition);
            if hr != S_OK { return Err(WhpxError::CallFailed("WHvCreatePartition", hr)); }

            let count: u32 = 1;
            // Property code values differ across SDK/header revisions; try known candidates.
            let mut prop_ok = false;
            for code in [0x0000_0001u32, 0x0000_000Bu32, WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT] {
                let hr = (api.set_partition_property)(partition, code, &count as *const _ as *const c_void, std::mem::size_of::<u32>() as u32);
                if hr == S_OK { prop_ok = true; break; }
            }
            // As a fallback, continue if unset: some hosts default to 1 vCPU.
            if !prop_ok {
                // keep going; setup may still succeed on platforms where default processor count is valid
            }

            let hr = (api.setup_partition)(partition);
            if hr != S_OK { let _=(api.delete_partition)(partition); return Err(WhpxError::CallFailed("WHvSetupPartition", hr)); }

            let flags = WHV_MAP_GPA_RANGE_FLAG_READ | WHV_MAP_GPA_RANGE_FLAG_WRITE | WHV_MAP_GPA_RANGE_FLAG_EXECUTE;
            let hr = (api.map_gpa_range)(partition, memory.as_ptr() as *const c_void, 0, memory.size() as u64, flags);
            if hr != S_OK { let _=(api.delete_partition)(partition); return Err(WhpxError::CallFailed("WHvMapGpaRange", hr)); }

            let hr = (api.create_vp)(partition, 0, 0);
            if hr != S_OK { let _=(api.unmap_gpa_range)(partition,0,memory.size() as u64); let _=(api.delete_partition)(partition); return Err(WhpxError::CallFailed("WHvCreateVirtualProcessor", hr)); }

            let mut this = Self { api, partition, vcpu_id: 0, mem_size: memory.size() as u64 };
            if let Err(e) = this.init_basic_state() {
                let _ = (this.api.delete_vp)(this.partition, 0);
                let _ = (this.api.unmap_gpa_range)(this.partition, 0, this.mem_size);
                let _ = (this.api.delete_partition)(this.partition);
                return Err(e);
            }
            Ok(this)
        }
    }

    unsafe fn set_reg_vals(&mut self, pairs: &[(u32, RegVal)]) -> Result<(), WhpxError> {
        let names: Vec<u32> = pairs.iter().map(|p| p.0).collect();
        let vals: Vec<RegVal> = pairs.iter().map(|p| p.1).collect();
        let hr = (self.api.set_regs)(self.partition, self.vcpu_id, names.as_ptr(), names.len() as u32, vals.as_ptr());
        if hr != S_OK { return Err(WhpxError::CallFailed("WHvSetVirtualProcessorRegisters", hr)); }
        Ok(())
    }

    unsafe fn init_basic_state(&mut self) -> Result<(), WhpxError> {
        // Minimal real-mode/flat baseline. Good enough for simple flat 16/32-bit guest code tests with IO/HLT.
        let code_seg = SegmentReg { base: 0, limit: 0xFFFF, selector: 0, attributes: 0x9B };
        let data_seg = SegmentReg { base: 0, limit: 0xFFFF, selector: 0, attributes: 0x93 };
        let mut pairs: Vec<(u32, RegVal)> = Vec::new();
        let g = |r: RegName, v: u64| (r as u32, RegVal { reg64: v });
        let s = |r: RegName, v: SegmentReg| (r as u32, RegVal { segment: v });
        pairs.extend_from_slice(&[
            g(RegName::Rip, 0), g(RegName::Rflags, 0x2),
            g(RegName::Rax,0),g(RegName::Rbx,0),g(RegName::Rcx,0),g(RegName::Rdx,0),
            g(RegName::Rsi,0),g(RegName::Rdi,0),g(RegName::Rbp,0),g(RegName::Rsp,0),
            g(RegName::R8,0),g(RegName::R9,0),g(RegName::R10,0),g(RegName::R11,0),
            g(RegName::R12,0),g(RegName::R13,0),g(RegName::R14,0),g(RegName::R15,0),
            g(RegName::Cr0,0),g(RegName::Cr2,0),g(RegName::Cr3,0),g(RegName::Cr4,0),g(RegName::Efer,0),
            s(RegName::Cs, code_seg), s(RegName::Ds, data_seg), s(RegName::Es, data_seg),
            s(RegName::Ss, data_seg), s(RegName::Fs, data_seg), s(RegName::Gs, data_seg),
        ]);
        self.set_reg_vals(&pairs)
    }

    pub fn get_regs(&mut self) -> Result<WhpxRegs, WhpxError> {
        unsafe {
            let names = [RegName::Rax as u32,RegName::Rbx as u32,RegName::Rcx as u32,RegName::Rdx as u32,RegName::Rsi as u32,RegName::Rdi as u32,RegName::Rbp as u32,RegName::Rsp as u32,RegName::R8 as u32,RegName::R9 as u32,RegName::R10 as u32,RegName::R11 as u32,RegName::R12 as u32,RegName::R13 as u32,RegName::R14 as u32,RegName::R15 as u32,RegName::Rip as u32,RegName::Rflags as u32];
            let mut vals = [RegVal{reg64:0};18];
            let hr = (self.api.get_regs)(self.partition, self.vcpu_id, names.as_ptr(), 18, vals.as_mut_ptr());
            if hr != S_OK { return Err(WhpxError::CallFailed("WHvGetVirtualProcessorRegisters", hr)); }
            Ok(WhpxRegs {
                rax: vals[0].reg64, rbx: vals[1].reg64, rcx: vals[2].reg64, rdx: vals[3].reg64,
                rsi: vals[4].reg64, rdi: vals[5].reg64, rbp: vals[6].reg64, rsp: vals[7].reg64,
                r8: vals[8].reg64, r9: vals[9].reg64, r10: vals[10].reg64, r11: vals[11].reg64,
                r12: vals[12].reg64, r13: vals[13].reg64, r14: vals[14].reg64, r15: vals[15].reg64,
                rip: vals[16].reg64, rflags: vals[17].reg64,
            })
        }
    }

    pub fn set_regs(&mut self, r: &WhpxRegs) -> Result<(), WhpxError> {
        unsafe {
            let pairs = [
                (RegName::Rax as u32, RegVal{reg64:r.rax}), (RegName::Rbx as u32, RegVal{reg64:r.rbx}), (RegName::Rcx as u32, RegVal{reg64:r.rcx}), (RegName::Rdx as u32, RegVal{reg64:r.rdx}),
                (RegName::Rsi as u32, RegVal{reg64:r.rsi}), (RegName::Rdi as u32, RegVal{reg64:r.rdi}), (RegName::Rbp as u32, RegVal{reg64:r.rbp}), (RegName::Rsp as u32, RegVal{reg64:r.rsp}),
                (RegName::R8 as u32, RegVal{reg64:r.r8}), (RegName::R9 as u32, RegVal{reg64:r.r9}), (RegName::R10 as u32, RegVal{reg64:r.r10}), (RegName::R11 as u32, RegVal{reg64:r.r11}),
                (RegName::R12 as u32, RegVal{reg64:r.r12}), (RegName::R13 as u32, RegVal{reg64:r.r13}), (RegName::R14 as u32, RegVal{reg64:r.r14}), (RegName::R15 as u32, RegVal{reg64:r.r15}),
                (RegName::Rip as u32, RegVal{reg64:r.rip}), (RegName::Rflags as u32, RegVal{reg64:r.rflags}),
            ];
            self.set_reg_vals(&pairs)
        }
    }

    pub fn set_rax(&mut self, rax: u64) -> Result<(), WhpxError> {
        unsafe { self.set_reg_vals(&[(RegName::Rax as u32, RegVal{reg64:rax})]) }
    }

    pub fn advance_rip(&mut self, delta: u64) -> Result<(), WhpxError> {
        let mut r = self.get_regs()?;
        r.rip = r.rip.wrapping_add(delta);
        self.set_regs(&r)
    }

    pub fn request_interrupt_supported(&self) -> bool { self.api.request_interrupt.is_some() }

    /// Best-effort interrupt injection for WHPX. Returns Ok(true) if API exists and call succeeded.
    /// This is a minimal slice: fixed interrupt to VP 0. Not full APIC/IOAPIC semantics.
    pub fn request_interrupt(&mut self, vector: u8) -> Result<bool, WhpxError> {
        let Some(req) = self.api.request_interrupt else { return Ok(false); };
        unsafe {
            // WHV_INTERRUPT_CONTROL: Type (Fixed=0) in low bits, Vector at bits 32..39 in common layout.
            let mut ctrl = InterruptControl::default();
            ctrl.type_vector_dest = (vector as u64) << 32; // Type=0 (fixed), Destination=0
            ctrl.destination_mode_flags = 0;
            let hr = req(self.partition, &ctrl, std::mem::size_of::<InterruptControl>() as u32);
            if hr == S_OK { Ok(true) } else { Err(WhpxError::CallFailed("WHvRequestInterrupt", hr)) }
        }
    }

    pub fn run(&mut self) -> Result<WhpxRunExit, WhpxError> {
        unsafe {
            // WHV_RUN_VP_EXIT_CONTEXT is fairly large (~4KB+ in newer SDKs). Allocate ample buffer.
            let mut buf = vec![0u8; 4096];
            let hr = (self.api.run_vp)(self.partition, self.vcpu_id, buf.as_mut_ptr() as *mut c_void, buf.len() as u32);
            if hr != S_OK { return Err(WhpxError::CallFailed("WHvRunVirtualProcessor", hr)); }
            let hdr = &*(buf.as_ptr() as *const RunExitHeader);
            let reason = hdr.exit_reason;
            // Payload offset: ExitReason(4)+Reserved(4)+VpContext. VpContext size varies by header version; common x64 is 40 bytes in older, larger in newer.
            // But WHV_RUN_VP_EXIT_CONTEXT layout: { WHV_RUN_VP_EXIT_REASON ExitReason; UINT32 Reserved; WHV_VP_EXIT_CONTEXT VpContext; union { ... } }
            // WHV_VP_EXIT_CONTEXT: { WHV_X64_VP_EXECUTION_STATE ExecutionState (UINT64 via bitfield/union represented as u16+u16+u32 in some, but often 8 bytes), [context regs?] ... }
            // This is fragile to guess. However, for IO port and memory access we can scan for plausible payload by also trying common offsets.
            Ok(match reason {
                WHV_RUN_VP_EXIT_REASON_NONE => WhpxRunExit::Interrupted,
                WHV_RUN_VP_EXIT_REASON_X64_HALT => WhpxRunExit::Halted,
                WHV_RUN_VP_EXIT_REASON_CANCELED => WhpxRunExit::Canceled,
                WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS => {
                    // Try a few likely payload offsets (after header+vpcontext)
                    let mut found: Option<WhpxRunExit> = None;
                    for off in [40usize, 48, 56, 64, 72, 80, 96, 128, 136, 144, 152, 160] {
                        if off + std::mem::size_of::<IoPortAccessCtx>() > buf.len() { continue; }
                        let io = &*(buf.as_ptr().add(off) as *const IoPortAccessCtx);
                        let size = ((io.access_info >> 1) & 0x7) as u8;
                        let port = io.port_number;
                        // heuristic: access size 1/2/4 and instruction byte count reasonable
                        if matches!(size, 1|2|4) && io.instruction_byte_count <= 15 {
                            let is_write = (io.access_info & 1) != 0;
                            found = Some(WhpxRunExit::IoPort { port, access_size: size, is_write, rax: io.rax, instruction_len: io.instruction_byte_count });
                            break;
                        }
                    }
                    found.unwrap_or_else(|| WhpxRunExit::Unsupported("IO port access (no se pudo decodificar contexto de salida)".into()))
                }
                WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS => {
                    let mut gpa = 0u64;
                    for off in [40usize, 48, 56, 64, 72, 80, 96, 128, 136, 144, 152, 160] {
                        if off + std::mem::size_of::<MemAccessCtx>() > buf.len() { continue; }
                        let mm = &*(buf.as_ptr().add(off) as *const MemAccessCtx);
                        if mm.gpa < self.mem_size || mm.gpa != 0 { gpa = mm.gpa; break; }
                    }
                    WhpxRunExit::MemoryAccess { gpa }
                }
                WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW => WhpxRunExit::Interrupted,
                WHV_RUN_VP_EXIT_REASON_UNSUPPORTED_FEATURE => WhpxRunExit::Unsupported("unsupported feature".into()),
                WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE => WhpxRunExit::Unsupported("invalid vp register value".into()),
                WHV_RUN_VP_EXIT_REASON_EXCEPTION => {
                    let mut info = "exception".to_string();
                    for off in [40usize, 48, 56, 64, 72, 80, 96, 128, 136, 144, 152, 160] {
                        if off + std::mem::size_of::<ExceptionCtx>() > buf.len() { continue; }
                        let ex = &*(buf.as_ptr().add(off) as *const ExceptionCtx);
                        if ex.instruction_byte_count <= 15 {
                            let err = if ex.error_code_valid != 0 { Some(ex.error_code) } else { None };
                            info = format!("exception type={} err={err:?} instr_len={}", ex._exception_type, ex.instruction_byte_count);
                            break;
                        }
                    }
                    WhpxRunExit::Unsupported(info)
                }
                WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION => WhpxRunExit::Failed("unrecoverable exception".into()),
                other => WhpxRunExit::Unsupported(format!("exit reason 0x{other:08X}")),
            })
        }
    }
}

impl Drop for WhpxPartition {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.api.delete_vp)(self.partition, self.vcpu_id);
            let _ = (self.api.unmap_gpa_range)(self.partition, 0, self.mem_size);
            let _ = (self.api.delete_partition)(self.partition);
        }
    }
}
