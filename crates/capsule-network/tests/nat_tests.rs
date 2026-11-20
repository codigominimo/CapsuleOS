use capsule_network::UserModeNat;

fn make_arp_request(sender_mac: [u8; 6], sender_ip: [u8; 4], target_ip: [u8; 4]) -> Vec<u8> {
    let mut frame = vec![0u8; 42];
    frame[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    frame[6..12].copy_from_slice(&sender_mac);
    frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes()); // ARP
    frame[14..16].copy_from_slice(&1u16.to_be_bytes()); // HTYPE Ethernet
    frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // PTYPE IPv4
    frame[18] = 6; // HLEN
    frame[19] = 4; // PLEN
    frame[20..22].copy_from_slice(&1u16.to_be_bytes()); // REQUEST
    frame[22..28].copy_from_slice(&sender_mac);
    frame[28..32].copy_from_slice(&sender_ip);
    frame[32..38].copy_from_slice(&[0u8; 6]);
    frame[38..42].copy_from_slice(&target_ip);
    frame
}

fn ipv4_checksum(header: &mut [u8]) {
    header[10] = 0;
    header[11] = 0;
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < header.len() {
        sum += u16::from_be_bytes([header[i], header[i + 1]]) as u32;
        i += 2;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let csum = !(sum as u16);
    header[10..12].copy_from_slice(&csum.to_be_bytes());
}

fn icmp_checksum(data: &mut [u8]) {
    data[2] = 0;
    data[3] = 0;
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let csum = !(sum as u16);
    data[2..4].copy_from_slice(&csum.to_be_bytes());
}

fn make_icmp_echo_request(src_mac: [u8; 6], src_ip: [u8; 4], dst_ip: [u8; 4], id: u16, seq: u16) -> Vec<u8> {
    let mut icmp = vec![0u8; 16];
    icmp[0] = 8; // echo request
    icmp[1] = 0;
    icmp[4..6].copy_from_slice(&id.to_be_bytes());
    icmp[6..8].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in icmp.iter_mut().enumerate().skip(8) {
        *b = (i as u8).wrapping_mul(3);
    }
    icmp_checksum(&mut icmp);

    let total_len = 20 + icmp.len();
    let mut frame = vec![0u8; 14 + total_len];
    frame[0..6].copy_from_slice(&[0x52, 0x55, 0x0A, 0x00, 0x02, 0x02]); // dst (gateway)
    frame[6..12].copy_from_slice(&src_mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let ip = &mut frame[14..];
    ip[0] = 0x45;
    ip[1] = 0;
    ip[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&0u16.to_be_bytes());
    ip[6..8].copy_from_slice(&0u16.to_be_bytes());
    ip[8] = 64;
    ip[9] = 1; // ICMP
    ip[12..16].copy_from_slice(&src_ip);
    ip[16..20].copy_from_slice(&dst_ip);
    let mut hdr = ip[..20].to_vec();
    ipv4_checksum(&mut hdr);
    ip[..20].copy_from_slice(&hdr);
    ip[20..20 + icmp.len()].copy_from_slice(&icmp);
    frame
}

#[test]
fn nat_replies_to_arp_request_for_gateway() {
    let guest_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let mut nat = UserModeNat::new(guest_mac);
    let req = make_arp_request(guest_mac, [10, 0, 2, 15], [10, 0, 2, 2]);
    nat.push_tx_frame(&req).expect("arp request accepted");

    let reply = nat.poll_rx_frame().expect("arp reply expected");
    assert_eq!(reply.len(), 42);
    assert_eq!(u16::from_be_bytes([reply[12], reply[13]]), 0x0806);
    assert_eq!(u16::from_be_bytes([reply[20], reply[21]]), 2, "ARP reply opcode");
    assert_eq!(&reply[28..32], &[10, 0, 2, 2], "sender protocol address (gateway)");
    assert_eq!(&reply[38..42], &[10, 0, 2, 15], "target protocol address (guest)");
}

#[test]
fn nat_replies_to_icmp_echo_to_gateway() {
    let guest_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let mut nat = UserModeNat::new(guest_mac);
    let req = make_icmp_echo_request(guest_mac, [10, 0, 2, 15], [10, 0, 2, 2], 0x1234, 1);
    nat.push_tx_frame(&req).expect("icmp request accepted");

    let reply = nat.poll_rx_frame().expect("icmp echo reply expected");
    assert!(reply.len() >= 14 + 20 + 8);
    assert_eq!(u16::from_be_bytes([reply[12], reply[13]]), 0x0800, "IPv4 ethertype");
    assert_eq!(reply[23], 1, "IP protocol ICMP");
    assert_eq!(&reply[26..30], &[10, 0, 2, 2], "src ip gateway");
    assert_eq!(&reply[30..34], &[10, 0, 2, 15], "dst ip guest");
    assert_eq!(reply[34], 0, "ICMP echo reply type");
    assert_eq!(reply[35], 0, "ICMP code");
}

#[test]
fn nat_replies_to_icmp_echo_to_dns_virtual_ip() {
    let guest_mac = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];
    let mut nat = UserModeNat::new(guest_mac);
    let req = make_icmp_echo_request(guest_mac, [10, 0, 2, 15], [10, 0, 2, 3], 0x0002, 7);
    nat.push_tx_frame(&req).expect("icmp request accepted");
    let reply = nat.poll_rx_frame().expect("icmp echo reply expected for virtual DNS IP");
    assert_eq!(reply[34], 0, "ICMP echo reply type");
    assert_eq!(&reply[26..30], &[10, 0, 2, 3], "src ip should be dns virtual ip");
}
