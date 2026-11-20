use capsule_devices::{PciBus, VirtioNetPci};

#[test]
fn virtio_net_pci_enumerates_at_00_03_0() {
    let mut pci = PciBus::new();
    pci.attach_device(Box::new(VirtioNetPci::new_default()));
    // Select bus 0, device 3, function 0, offset 0 (Vendor/Device)
    pci.write_io(0xCF8, 0x8000_1800, 4);
    let id = pci.read_io(0xCFC, 4);
    assert_eq!(id & 0xFFFF, 0x1AF4, "virtio vendor id");
    assert_eq!((id >> 16) & 0xFFFF, 0x1000, "virtio-net transitional device id");
}

#[test]
fn virtio_net_bar0_io_exposes_mac_after_io_enable() {
    let mut pci = PciBus::new();
    pci.attach_device(Box::new(VirtioNetPci::new_default()));

    // Enable I/O space decoding (command bit 0) for 00:03.0
    pci.write_io(0xCF8, 0x8000_1804, 4); // command/status dword
    let cmdsts = pci.read_io(0xCFC, 4);
    pci.write_io(0xCFC, cmdsts | 0x0001, 4);

    // Read BAR0
    pci.write_io(0xCF8, 0x8000_1810, 4);
    let bar0 = pci.read_io(0xCFC, 4);
    assert_eq!(bar0 & 0x1, 0x1, "BAR0 must be I/O space");
    let bar0_base = (bar0 & 0xFFFC) as u16;
    assert_ne!(bar0_base, 0, "BAR0 base must be programmed");

    // Virtio-net device config starts at BAR+0x14 (legacy), MAC is first 6 bytes
    let mut mac = [0u8; 6];
    for i in 0..6u16 {
        mac[i as usize] = pci.read_io(bar0_base + 0x14 + i, 1) as u8;
    }
    assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
}

#[test]
fn virtio_net_ignores_io_when_command_io_disabled() {
    let mut pci = PciBus::new();
    pci.attach_device(Box::new(VirtioNetPci::new_default()));

    // Ensure I/O decoding disabled (default 0)
    pci.write_io(0xCF8, 0x8000_1810, 4);
    let bar0 = pci.read_io(0xCFC, 4);
    let bar0_base = (bar0 & 0xFFFC) as u16;
    assert_ne!(bar0_base, 0);

    // Reads from BAR should not decode virtio regs when command.IO is clear
    let v = pci.read_io(bar0_base + 0x14, 1);
    assert_eq!(v & 0xFF, 0xFF, "BAR I/O must be ignored when PCI command IO bit is clear");
}
