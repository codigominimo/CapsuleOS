use capsule_core::{ByteSize, HostInfo, WhpxStatus};

#[test]
fn test_host_os_detection() {
    let host = HostInfo::detect();
    assert!(!host.os.os_name.is_empty(), "OS name should not be empty");
    assert_eq!(host.os.architecture, "x86_64", "Target architecture should be x86_64");
    println!("Detected OS: {} (Build {})", host.os.os_name, host.os.build_number);
}

#[test]
fn test_host_cpu_detection() {
    let host = HostInfo::detect();
    assert!(!host.cpu.vendor.is_empty(), "CPU vendor should not be empty");
    assert!(!host.cpu.brand_name.is_empty(), "CPU brand name should not be empty");
    assert!(host.cpu.logical_cores > 0, "Logical cores must be > 0");
    println!(
        "Detected CPU: {} (Vendor: {}, Cores: {}, VT-x/SVM: {})",
        host.cpu.brand_name,
        host.cpu.vendor,
        host.cpu.logical_cores,
        host.cpu.virtualization_supported()
    );
}

#[test]
fn test_host_memory_detection() {
    let host = HostInfo::detect();
    assert!(host.memory.total_phys.as_bytes() > 0, "Total RAM should be > 0");
    assert!(host.memory.avail_phys.as_bytes() > 0, "Available RAM should be > 0");
    println!(
        "Detected RAM: Total {}, Available {}, Load {}%",
        host.memory.total_phys,
        host.memory.avail_phys,
        host.memory.memory_load_percent
    );
}

#[test]
fn test_host_whpx_detection() {
    let host = HostInfo::detect();
    println!("Detected WHPX: {:?} ({})", host.whpx, host.whpx.description());
    // On Windows 10/11 x64, it should be either Available, FeatureNotEnabled, or NotInstalled
    match host.whpx {
        WhpxStatus::Available => println!("-> WHPX Hardware Acceleration is ACTIVE!"),
        WhpxStatus::FeatureNotEnabled => println!("-> WinHvPlatform is present but Hypervisor is not active"),
        WhpxStatus::NotInstalled => println!("-> WinHvPlatform is not installed"),
    }
}

#[test]
fn test_bytesize_formatting() {
    let b = ByteSize::from_gib(4);
    assert_eq!(format!("{}", b), "4.00 GB");

    let b2 = ByteSize::from_mib(512);
    assert_eq!(format!("{}", b2), "512.0 MB");
}
