//! Dispositivo de red virtio-net (transitional, transporte PCI legacy / I/O BAR)
//! integrado con una pila de red en modo usuario (NAT) para CapsuleOS (M8).
//!
//! Implementación mínima orientada a pruebas con la vCPU software (sin MMIO ni inyección
//! de interrupciones): usa BAR0 de E/S (I/O ports), colas RX/TX (split virtqueues) en
//! memoria guest y sondeo desde el bucle de la VM.

use crate::pci::PciDevice;
use capsule_memory::GuestMemory;
use capsule_network::UserModeNat;
use std::any::Any;

const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_NET_DEVICE_ID: u16 = 0x1000; // transitional network card
const VIRTIO_SUBSYSTEM_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_SUBSYSTEM_DEVICE_ID: u16 = 0x0001;

const VIRTIO_NET_F_MAC: u32 = 1 << 5;
const VIRTIO_NET_F_STATUS: u32 = 1 << 16;

#[allow(dead_code)]
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
#[allow(dead_code)]
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
#[allow(dead_code)]
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

const VIRTIO_PCI_HOST_FEATURES: u16 = 0x00;
const VIRTIO_PCI_GUEST_FEATURES: u16 = 0x04;
const VIRTIO_PCI_QUEUE_PFN: u16 = 0x08;
const VIRTIO_PCI_QUEUE_NUM: u16 = 0x0C;
const VIRTIO_PCI_QUEUE_SEL: u16 = 0x0E;
const VIRTIO_PCI_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_PCI_STATUS: u16 = 0x12;
const VIRTIO_PCI_ISR: u16 = 0x13;
const VIRTIO_PCI_CONFIG_START: u16 = 0x14;

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

const VIRTIO_NET_HDR_LEN: usize = 10; // sizeof(struct virtio_net_hdr) legacy

#[derive(Clone, Copy, Debug, Default)]
struct QueueState {
    size: u16,
    pfn: u32,
    ready: bool,
    last_avail_idx: u16,
}

impl QueueState {
    #[inline]
    fn desc_addr(&self) -> u64 {
        (self.pfn as u64) << 12
    }
    #[inline]
    fn avail_addr(&self) -> u64 {
        self.desc_addr() + (self.size as u64) * 16
    }
    #[inline]
    fn used_addr(&self) -> u64 {
        let avail_end = self.avail_addr() + 4 + (self.size as u64) * 2;
        // legacy virtqueue: used ring aligned to 4096
        (avail_end + 4095) & !4095
    }
}

#[derive(Debug)]
pub struct VirtioNetPci {
    bdf: (u8, u8, u8),
    config: [u8; 256],
    bar0_base: u16,
    bar0_size: u16,
    device_features: u32,
    guest_features: u32,
    queue_select: u16,
    device_status: u8,
    isr_status: u8,
    queues: [QueueState; 2], // 0=RX, 1=TX
    mac: [u8; 6],
    link_up: bool,
    nat: UserModeNat,
}

impl VirtioNetPci {
    pub fn new_default() -> Self {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        Self::new((0, 3, 0), mac)
    }

    pub fn new(bdf: (u8, u8, u8), mac: [u8; 6]) -> Self {
        let mut config = [0u8; 256];
        // Vendor/Device
        config[0x00..0x02].copy_from_slice(&VIRTIO_VENDOR_ID.to_le_bytes());
        config[0x02..0x04].copy_from_slice(&VIRTIO_NET_DEVICE_ID.to_le_bytes());
        // Status/Command: IO space capable (bit 0), but decoding disabled until BAR programmed/command set
        config[0x04..0x06].copy_from_slice(&0x0000u16.to_le_bytes()); // command
        config[0x06..0x08].copy_from_slice(&0x0010u16.to_le_bytes()); // status: capabilities list
        config[0x08] = 0x00; // revision
        config[0x09] = 0x00; // prog IF
        config[0x0A] = 0x00; // subclass (ethernet)
        config[0x0B] = 0x02; // class (network controller)
        config[0x0E] = 0x00; // header type
        // Subsystem vendor/device (common for virtio)
        config[0x2C..0x2E].copy_from_slice(&VIRTIO_SUBSYSTEM_VENDOR_ID.to_le_bytes());
        config[0x2E..0x30].copy_from_slice(&VIRTIO_SUBSYSTEM_DEVICE_ID.to_le_bytes());
        // BAR0 (I/O). We'll program a default base for convenience.
        let bar0_base: u16 = 0xC040;
        config[0x10] = 0x01 | ((bar0_base as u8) & 0xFC); // IO space indicator
        config[0x11] = (bar0_base >> 8) as u8;
        // Interrupt line/pin (INTA# -> IRQ 11 por defecto, puede reprogramarse por guest)
        config[0x3C] = 11;
        config[0x3D] = 1; // INTA#

        let mut queues = [QueueState::default(), QueueState::default()];
        queues[0].size = 256; // RX
        queues[1].size = 256; // TX

        Self {
            bdf,
            config,
            bar0_base,
            bar0_size: 0x20, // legacy common + net config
            device_features: VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS,
            guest_features: 0,
            queue_select: 0,
            device_status: 0,
            isr_status: 0,
            queues,
            mac,
            link_up: true,
            nat: UserModeNat::new(mac),
        }
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    #[inline]
    fn command(&self) -> u16 {
        u16::from_le_bytes([self.config[0x04], self.config[0x05]])
    }

    #[inline]
    fn io_enabled(&self) -> bool {
        (self.command() & 0x0001) != 0
    }

    #[inline]
    #[allow(dead_code)]
    fn selected_queue_mut(&mut self) -> &mut QueueState {
        let idx = if self.queue_select == 0 { 0 } else { 1 };
        &mut self.queues[idx]
    }

    fn bar0_offset(&self, port: u16) -> Option<u16> {
        if !self.io_enabled() || self.bar0_base == 0 {
            return None;
        }
        if port >= self.bar0_base && port < self.bar0_base.saturating_add(self.bar0_size) {
            Some(port - self.bar0_base)
        } else {
            None
        }
    }

    fn read_config_u32(&self, aligned_off: usize) -> u32 {
        let b = &self.config[aligned_off..aligned_off + 4];
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn write_config_u32(&mut self, aligned_off: usize, value: u32) {
        match aligned_off {
            0x04 => {
                // command/status (status high word is RO in general; keep low 16 writable for simplicity)
                let cmd = (value & 0xFFFF) as u16;
                self.config[0x04..0x06].copy_from_slice(&cmd.to_le_bytes());
            }
            0x10 => {
                // BAR0 I/O
                let new_base = (value as u16) & 0xFFFC;
                self.bar0_base = new_base;
                self.config[0x10] = 0x01 | ((new_base as u8) & 0xFC);
                self.config[0x11] = (new_base >> 8) as u8;
                self.config[0x12] = 0;
                self.config[0x13] = 0;
            }
            0x3C => {
                self.config[0x3C] = (value & 0xFF) as u8; // interrupt line (RW)
            }
            _ => {
                // otros registros de config ignorados/escritura directa limitada
                if aligned_off + 4 <= self.config.len() {
                    self.config[aligned_off..aligned_off + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
        }
    }

    fn read_bar(&mut self, offset: u16, size: u8) -> u32 {
        let read_u8 = |this: &mut Self, off: u16| -> u8 {
            match off {
                VIRTIO_PCI_STATUS => this.device_status,
                VIRTIO_PCI_ISR => {
                    let v = this.isr_status;
                    this.isr_status = 0; // read-to-clear
                    v
                }
                o if o >= VIRTIO_PCI_CONFIG_START => {
                    let idx = (o - VIRTIO_PCI_CONFIG_START) as usize;
                    if idx < 6 { this.mac[idx] } else if idx == 6 { this.link_up as u8 } else if idx == 7 { 0 } else { 0 }
                }
                _ => 0,
            }
        };
        let read_u16 = |this: &mut Self, off: u16| -> u16 {
            match off {
                VIRTIO_PCI_QUEUE_NUM => this.queues[if this.queue_select == 0 { 0 } else { 1 }].size,
                VIRTIO_PCI_QUEUE_SEL => this.queue_select,
                VIRTIO_PCI_QUEUE_NOTIFY => 0,
                VIRTIO_PCI_HOST_FEATURES => (this.device_features & 0xFFFF) as u16,
                VIRTIO_PCI_GUEST_FEATURES => (this.guest_features & 0xFFFF) as u16,
                VIRTIO_PCI_QUEUE_PFN => (this.queues[if this.queue_select == 0 { 0 } else { 1 }].pfn & 0xFFFF) as u16,
                VIRTIO_PCI_STATUS => this.device_status as u16,
                VIRTIO_PCI_ISR => read_u8(this, VIRTIO_PCI_ISR) as u16,
                o if o >= VIRTIO_PCI_CONFIG_START => {
                    let lo = read_u8(this, o) as u16;
                    let hi = read_u8(this, o + 1) as u16;
                    lo | (hi << 8)
                }
                _ => u16::from_le_bytes([read_u8(this, off), read_u8(this, off + 1)]),
            }
        };
        match size {
            1 => read_u8(self, offset) as u32,
            2 => read_u16(self, offset) as u32,
            4 => match offset {
                VIRTIO_PCI_HOST_FEATURES => self.device_features,
                VIRTIO_PCI_GUEST_FEATURES => self.guest_features,
                VIRTIO_PCI_QUEUE_PFN => self.queues[if self.queue_select == 0 { 0 } else { 1 }].pfn,
                _ => {
                    let lo = read_u16(self, offset) as u32;
                    let hi = read_u16(self, offset + 2) as u32;
                    lo | (hi << 16)
                }
            },
            _ => read_u8(self, offset) as u32,
        }
    }

    fn write_bar(&mut self, offset: u16, value: u32, size: u8) {
        let v8 = (value & 0xFF) as u8;
        let v16 = (value & 0xFFFF) as u16;
        match offset {
            VIRTIO_PCI_GUEST_FEATURES => {
                if size >= 4 { self.guest_features = value; } else { self.guest_features = (self.guest_features & 0xFFFF_0000) | v16 as u32; }
            }
            VIRTIO_PCI_QUEUE_PFN => {
                let qidx = if self.queue_select == 0 { 0 } else { 1 };
                let pfn = if size >= 4 { value } else { (self.queues[qidx].pfn & 0xFFFF_0000) | v16 as u32 };
                self.queues[qidx].pfn = pfn;
                self.queues[qidx].ready = pfn != 0;
                self.queues[qidx].last_avail_idx = 0;
            }
            VIRTIO_PCI_QUEUE_SEL => {
                self.queue_select = v16;
            }
            VIRTIO_PCI_QUEUE_NOTIFY => {
                // El procesamiento real se realiza en poll(mem) con acceso a memoria guest.
                let _ = v16;
            }
            VIRTIO_PCI_STATUS => {
                self.device_status = v8;
                if v8 == 0 {
                    // reset
                    self.guest_features = 0;
                    self.queue_select = 0;
                    self.isr_status = 0;
                    for q in &mut self.queues {
                        q.pfn = 0;
                        q.ready = false;
                        q.last_avail_idx = 0;
                    }
                }
            }
            _ => {
                // escrituras a config de dispositivo (MAC) ignoradas en este modelo
                let _ = (offset, size, value);
            }
        }
    }

    fn can_process(&self) -> bool {
        self.device_status & VIRTIO_STATUS_DRIVER_OK != 0
    }

    pub fn poll(&mut self, mem: &mut GuestMemory) {
        // Avanzar backend NAT (recepción UDP, timers básicos)
        if let Some(frame) = self.nat.poll_rx_frame() {
            self.enqueue_rx_frame(mem, &frame);
        }
        if !self.can_process() {
            return;
        }
        self.process_tx_queue(mem);
        // Intentar entregar frames RX que hayan llegado tras procesar TX
        let mut spins = 0;
        while let Some(frame) = self.nat.poll_rx_frame() {
            self.enqueue_rx_frame(mem, &frame);
            spins += 1;
            if spins > 32 { break; }
        }
    }

    fn process_tx_queue(&mut self, mem: &mut GuestMemory) {
        let qidx = 1usize;
        let (size, pfn, ready) = (self.queues[qidx].size, self.queues[qidx].pfn, self.queues[qidx].ready);
        if !ready || pfn == 0 || size == 0 { return; }
        let desc_base = self.queues[qidx].desc_addr();
        let avail_base = self.queues[qidx].avail_addr();
        let used_base = self.queues[qidx].used_addr();

        let avail_idx = read_u16_mem(mem, avail_base + 2).unwrap_or(self.queues[qidx].last_avail_idx);
        let mut last = self.queues[qidx].last_avail_idx;
        let mut budget = 64u32;
        while last != avail_idx && budget > 0 {
            budget -= 1;
            let ring_index = (last as u64) % (size as u64);
            let head = read_u16_mem(mem, avail_base + 4 + ring_index * 2).unwrap_or(0);
            let frame = self.read_tx_chain(mem, desc_base, size, head);
            if let Some(frame) = frame {
                let _ = self.nat.push_tx_frame(&frame);
            }
            // TX: no bytes escritos al guest, len 0
            let used_idx = read_u16_mem(mem, used_base + 2).unwrap_or(0);
            let used_slot = used_base + 4 + ((used_idx as u64) % (size as u64)) * 8;
            let _ = write_u32_mem(mem, used_slot, head as u32);
            let _ = write_u32_mem(mem, used_slot + 4, 0);
            let _ = write_u16_mem(mem, used_base + 2, used_idx.wrapping_add(1));
            self.isr_status |= 0x1;
            last = last.wrapping_add(1);
        }
        self.queues[qidx].last_avail_idx = last;
    }

    fn read_tx_chain(&self, mem: &GuestMemory, desc_base: u64, qsize: u16, head: u16) -> Option<Vec<u8>> {
        let mut idx = head;
        let mut out = Vec::new();
        for _ in 0..qsize {
            let d = desc_base + (idx as u64) * 16;
            let addr = read_u64_mem(mem, d).unwrap_or(0);
            let len = read_u32_mem(mem, d + 8).unwrap_or(0) as usize;
            let flags = read_u16_mem(mem, d + 12).unwrap_or(0);
            let next = read_u16_mem(mem, d + 14).unwrap_or(0);
            if addr != 0 && len != 0 {
                let mut buf = vec![0u8; len];
                if mem.read_bytes(capsule_core::types::GuestAddress::new(addr), &mut buf).is_ok() {
                    out.extend_from_slice(&buf);
                }
            }
            if flags & VRING_DESC_F_NEXT == 0 { break; }
            idx = next;
        }
        if out.len() <= VIRTIO_NET_HDR_LEN {
            return None;
        }
        Some(out[VIRTIO_NET_HDR_LEN..].to_vec())
    }

    fn enqueue_rx_frame(&mut self, mem: &mut GuestMemory, frame: &[u8]) {
        let qidx = 0usize;
        let (size, pfn, ready) = (self.queues[qidx].size, self.queues[qidx].pfn, self.queues[qidx].ready);
        if !ready || pfn == 0 || size == 0 || !self.can_process() { return; }
        let desc_base = self.queues[qidx].desc_addr();
        let avail_base = self.queues[qidx].avail_addr();
        let used_base = self.queues[qidx].used_addr();
        let avail_idx = read_u16_mem(mem, avail_base + 2).unwrap_or(self.queues[qidx].last_avail_idx);
        let last = self.queues[qidx].last_avail_idx;
        if last == avail_idx {
            return; // sin buffers RX disponibles
        }
        let ring_index = (last as u64) % (size as u64);
        let head = read_u16_mem(mem, avail_base + 4 + ring_index * 2).unwrap_or(0);

        // Construir paquete con cabecera virtio_net_hdr (10 bytes, cero) + frame Ethernet
        let mut packet = vec![0u8; VIRTIO_NET_HDR_LEN + frame.len()];
        packet[VIRTIO_NET_HDR_LEN..].copy_from_slice(frame);

        let written = self.write_rx_chain(mem, desc_base, size, head, &packet);
        let used_idx = read_u16_mem(mem, used_base + 2).unwrap_or(0);
        let used_slot = used_base + 4 + ((used_idx as u64) % (size as u64)) * 8;
        let _ = write_u32_mem(mem, used_slot, head as u32);
        let _ = write_u32_mem(mem, used_slot + 4, written as u32);
        let _ = write_u16_mem(mem, used_base + 2, used_idx.wrapping_add(1));
        self.queues[qidx].last_avail_idx = last.wrapping_add(1);
        self.isr_status |= 0x1;
    }

    fn write_rx_chain(&self, mem: &mut GuestMemory, desc_base: u64, qsize: u16, head: u16, data: &[u8]) -> usize {
        let mut idx = head;
        let mut remaining = data;
        let mut total = 0usize;
        for _ in 0..qsize {
            let d = desc_base + (idx as u64) * 16;
            let addr = read_u64_mem(mem, d).unwrap_or(0);
            let len = read_u32_mem(mem, d + 8).unwrap_or(0) as usize;
            let flags = read_u16_mem(mem, d + 12).unwrap_or(0);
            let next = read_u16_mem(mem, d + 14).unwrap_or(0);
            if addr != 0 && len != 0 && (flags & VRING_DESC_F_WRITE) != 0 && !remaining.is_empty() {
                let take = remaining.len().min(len);
                let _ = mem.write_bytes(capsule_core::types::GuestAddress::new(addr), &remaining[..take]);
                total += take;
                remaining = &remaining[take..];
            }
            if flags & VRING_DESC_F_NEXT == 0 { break; }
            idx = next;
            if remaining.is_empty() { break; }
        }
        total
    }
}

impl PciDevice for VirtioNetPci {
    fn bdf(&self) -> (u8, u8, u8) { self.bdf }

    fn config_read(&mut self, offset: u8, size: u8) -> u32 {
        let aligned = (offset & !0x3) as usize;
        let shift = ((offset & 0x3) * 8) as u32;
        let val = self.read_config_u32(aligned);
        match size {
            1 => (val >> shift) & 0xFF,
            2 => (val >> shift) & 0xFFFF,
            4 => val,
            _ => (val >> shift) & 0xFF,
        }
    }

    fn config_write(&mut self, offset: u8, value: u32, size: u8) {
        // aplicar write por bytes/lane sobre la dword alineada cuando corresponde
        let aligned = (offset & !0x3) as usize;
        let cur = self.read_config_u32(aligned);
        let shift = ((offset & 0x3) * 8) as u32;
        let new_val = match size {
            1 => (cur & !(0xFFu32 << shift)) | (((value & 0xFF) << shift)),
            2 => (cur & !(0xFFFFu32 << shift)) | (((value & 0xFFFF) << shift)),
            4 => value,
            _ => (cur & !(0xFFu32 << shift)) | (((value & 0xFF) << shift)),
        };
        self.write_config_u32(aligned, new_val);
    }

    fn handles_io_port(&self, port: u16) -> bool {
        self.bar0_offset(port).is_some()
    }

    fn io_read(&mut self, port: u16, size: u8) -> u32 {
        if let Some(off) = self.bar0_offset(port) {
            self.read_bar(off, size)
        } else {
            0xFF
        }
    }

    fn io_write(&mut self, port: u16, data: u32, size: u8) {
        if let Some(off) = self.bar0_offset(port) {
            self.write_bar(off, data, size);
        }
    }

    fn poll(&mut self, mem: &mut GuestMemory) {
        VirtioNetPci::poll(self, mem);
    }

    fn irq_level(&self) -> bool {
        // INTx nivelada mientras ISR tenga bits pendientes (modelo simplificado legacy)
        self.isr_status != 0
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

fn read_u16_mem(mem: &GuestMemory, addr: u64) -> Option<u16> {
    let mut b = [0u8; 2];
    mem.read_bytes(capsule_core::types::GuestAddress::new(addr), &mut b).ok()?;
    Some(u16::from_le_bytes(b))
}
fn read_u32_mem(mem: &GuestMemory, addr: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    mem.read_bytes(capsule_core::types::GuestAddress::new(addr), &mut b).ok()?;
    Some(u32::from_le_bytes(b))
}
fn read_u64_mem(mem: &GuestMemory, addr: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    mem.read_bytes(capsule_core::types::GuestAddress::new(addr), &mut b).ok()?;
    Some(u64::from_le_bytes(b))
}
fn write_u16_mem(mem: &mut GuestMemory, addr: u64, val: u16) -> Option<()> {
    mem.write_bytes(capsule_core::types::GuestAddress::new(addr), &val.to_le_bytes()).ok()
}
fn write_u32_mem(mem: &mut GuestMemory, addr: u64, val: u32) -> Option<()> {
    mem.write_bytes(capsule_core::types::GuestAddress::new(addr), &val.to_le_bytes()).ok()
}
