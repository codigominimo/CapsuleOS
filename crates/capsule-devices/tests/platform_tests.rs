use capsule_devices::{PciBus, Pic8259, Pit8254, RtcCmos};

#[test]
fn pic8259_initialization_and_imr() {
    let mut pic = Pic8259::new();
    // ICW1: init, expect ICW4
    pic.write_io(0x20, 0x11, 1);
    pic.write_io(0xA0, 0x11, 1);
    // ICW2: vector bases
    pic.write_io(0x21, 0x20, 1); // master -> 0x20-0x27
    pic.write_io(0xA1, 0x28, 1); // slave  -> 0x28-0x2F
    // ICW3: cascade wiring
    pic.write_io(0x21, 0x04, 1); // slave on IRQ2
    pic.write_io(0xA1, 0x02, 1); // slave identity
    // ICW4: 8086 mode
    pic.write_io(0x21, 0x01, 1);
    pic.write_io(0xA1, 0x01, 1);
    // OCW1: unmask all
    pic.write_io(0x21, 0x00, 1);
    pic.write_io(0xA1, 0x00, 1);

    assert_eq!(pic.master_imr(), 0x00);
    assert_eq!(pic.slave_imr(), 0x00);

    // Mask IRQ0 on master
    pic.write_io(0x21, 0x01, 1);
    assert_eq!(pic.master_imr(), 0x01);
}

#[test]
fn pic8259_irq_ack_and_eoi() {
    let mut pic = Pic8259::new();
    pic.write_io(0x20, 0x11, 1);
    pic.write_io(0xA0, 0x11, 1);
    pic.write_io(0x21, 0x20, 1);
    pic.write_io(0xA1, 0x28, 1);
    pic.write_io(0x21, 0x04, 1);
    pic.write_io(0xA1, 0x02, 1);
    pic.write_io(0x21, 0x01, 1);
    pic.write_io(0xA1, 0x01, 1);
    pic.write_io(0x21, 0x00, 1);
    pic.write_io(0xA1, 0x00, 1);

    pic.raise_irq(0); // IRQ0
    assert!(pic.has_pending_interrupt());
    let vec = pic.get_interrupt().expect("IRQ0 vector");
    assert_eq!(vec, 0x20);
    assert_ne!(pic.master_isr() & 0x01, 0, "IRQ0 debe quedar en servicio");
    // EOI no específico al maestro
    pic.write_io(0x20, 0x20, 1);
    assert_eq!(pic.master_isr() & 0x01, 0, "EOI debe limpiar ISR de IRQ0");
}

#[test]
fn pic8259_cascade_irq14_from_slave() {
    let mut pic = Pic8259::new();
    pic.write_io(0x20, 0x11, 1);
    pic.write_io(0xA0, 0x11, 1);
    pic.write_io(0x21, 0x20, 1);
    pic.write_io(0xA1, 0x28, 1);
    pic.write_io(0x21, 0x04, 1);
    pic.write_io(0xA1, 0x02, 1);
    pic.write_io(0x21, 0x01, 1);
    pic.write_io(0xA1, 0x01, 1);
    pic.write_io(0x21, 0x00, 1);
    pic.write_io(0xA1, 0x00, 1);

    pic.raise_irq(14); // slave IRQ6
    assert!(pic.has_pending_interrupt(), "IRQ14 debe propagarse por cascada");
    let vec = pic.get_interrupt().expect("IRQ14 vector");
    assert_eq!(vec, 0x2E, "IRQ14 debe mapear a vector base esclavo + 6");
    // EOI específico esclavo y luego maestro IRQ2 en servicio
    pic.write_io(0xA0, 0x20, 1);
    pic.write_io(0x20, 0x20, 1);
    assert_eq!(pic.slave_isr(), 0);
    assert_eq!(pic.master_isr(), 0);
}

#[test]
fn pit8254_channel0_mode2_raises_irq0_pulse() {
    let mut pit = Pit8254::new();
    // CH0, lobyte/hibyte, mode 2, binary
    pit.write_io(0x43, 0x34, 1);
    pit.write_io(0x40, 0x04, 1); // low
    pit.write_io(0x40, 0x00, 1); // high => reload 4

    let mut pulses = 0u32;
    for _ in 0..12 {
        if pit.tick(1) {
            pulses += 1;
        }
    }
    assert!(pulses >= 1, "PIT canal 0 (modo 2) debe generar al menos un pulso IRQ0");
}

#[test]
fn rtc_cmos_reports_time_and_preserves_registers() {
    let mut rtc = RtcCmos::new();
    // seconds
    rtc.write_io(0x70, 0x00, 1);
    let sec = rtc.read_io(0x71, 1) & 0xFF;
    assert!(sec <= 59, "segundos RTC fuera de rango: {sec}");
    // minutes
    rtc.write_io(0x70, 0x02, 1);
    let min = rtc.read_io(0x71, 1) & 0xFF;
    assert!(min <= 59, "minutos RTC fuera de rango: {min}");
    // hours (24h)
    rtc.write_io(0x70, 0x04, 1);
    let hour = rtc.read_io(0x71, 1) & 0xFF;
    assert!(hour <= 23, "hora RTC fuera de rango: {hour}");

    // Registro de propósito general CMOS (0x10) debe conservar escritura
    rtc.write_io(0x70, 0x10, 1);
    rtc.write_io(0x71, 0xA5, 1);
    rtc.write_io(0x70, 0x10, 1);
    let v = rtc.read_io(0x71, 1) & 0xFF;
    assert_eq!(v, 0xA5, "CMOS RAM debe conservar valor escrito");
}

#[test]
fn pci_config_space_mechanism1_address_and_empty_bus() {
    let mut pci = PciBus::new();
    assert!(pci.handles_port(0xCF8));
    assert!(pci.handles_port(0xCFC));

    // Escribir dirección de configuración (enable + bus/dev/fn/reg)
    pci.write_io(0xCF8, 0x8000_0000, 4);
    let addr = pci.read_io(0xCF8, 4);
    assert_eq!(addr, 0x8000_0000, "Registro 0xCF8 debe conservar la dirección");

    // Bus vacío: Vendor/Device ID inexistente
    let vendor = pci.read_io(0xCFC, 2);
    assert_eq!(vendor, 0xFFFF, "Bus PCI vacío debe devolver 0xFFFF en Vendor ID");
    let val32 = pci.read_io(0xCFC, 4);
    assert_eq!(val32, 0xFFFF_FFFF, "Bus PCI vacío debe devolver 0xFFFF_FFFF");

    // Deshabilitar enable bit -> lecturas de datos deben devolver FF
    pci.write_io(0xCF8, 0x0000_0000, 4);
    assert_eq!(pci.read_io(0xCFC, 1), 0xFF);
}
