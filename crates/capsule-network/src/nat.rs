use crate::NetworkError;
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const ETH_TYPE_ARP: u16 = 0x0806;
const ETH_TYPE_IPV4: u16 = 0x0800;
const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;
const IP_PROTO_ICMP: u8 = 1;
const IP_PROTO_UDP: u8 = 17;
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

/// NAT/stack de red en modo usuario (simplificado, M8).
/// - Responde ARP para la puerta de enlace
/// - Proporciona DHCP básico
/// - Responde ICMP echo al gateway/guest
/// - Reenvía UDP saliente mediante sockets del host y retorna respuestas
#[derive(Debug)]
pub struct UserModeNat {
    guest_mac: [u8; 6],
    gateway_mac: [u8; 6],
    guest_ip: Ipv4Addr,
    gateway_ip: Ipv4Addr,
    netmask: Ipv4Addr,
    dns_ip: Ipv4Addr,
    dhcp_offered: bool,
    dhcp_acked: bool,
    udp_socket: Option<UdpSocket>,
    pending_udp: Option<PendingUdp>,
    rx_queue: VecDeque<Vec<u8>>,
    last_poll: Instant,
}

#[derive(Debug, Clone)]
struct PendingUdp {
    guest_port: u16,
    dst_ip: Ipv4Addr,
    dst_port: u16,
    started: Instant,
}

impl Default for UserModeNat {
    fn default() -> Self {
        Self::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
    }
}

impl UserModeNat {
    pub fn new(guest_mac: [u8; 6]) -> Self {
        let udp_socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).ok();
        if let Some(s) = &udp_socket {
            let _ = s.set_nonblocking(true);
            let _ = s.set_read_timeout(Some(Duration::from_millis(1)));
        }
        Self {
            guest_mac,
            gateway_mac: [0x52, 0x55, 0x0A, 0x00, 0x02, 0x02],
            guest_ip: Ipv4Addr::new(10, 0, 2, 15),
            gateway_ip: Ipv4Addr::new(10, 0, 2, 2),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            dns_ip: Ipv4Addr::new(10, 0, 2, 3),
            dhcp_offered: false,
            dhcp_acked: false,
            udp_socket,
            pending_udp: None,
            rx_queue: VecDeque::new(),
            last_poll: Instant::now(),
        }
    }

    pub fn guest_mac(&self) -> [u8; 6] {
        self.guest_mac
    }

    pub fn push_tx_frame(&mut self, frame: &[u8]) -> Result<(), NetworkError> {
        if frame.len() < 14 {
            return Err(NetworkError::InvalidFrame("frame Ethernet demasiado corto".into()));
        }
        // Si el guest usa otra MAC origen, aprenderla (útil para tests/synthetic guests)
        self.guest_mac.copy_from_slice(&frame[6..12]);
        let eth_type = u16::from_be_bytes([frame[12], frame[13]]);
        match eth_type {
            ETH_TYPE_ARP => self.handle_arp(frame),
            ETH_TYPE_IPV4 => self.handle_ipv4(frame),
            _ => Ok(()),
        }
    }

    pub fn poll_rx_frame(&mut self) -> Option<Vec<u8>> {
        self.poll_udp_socket();
        self.rx_queue.pop_front()
    }

    fn poll_udp_socket(&mut self) {
        // Evitar trabajo excesivo en bucles ajustados
        if self.last_poll.elapsed() < Duration::from_millis(1) {
            // aún así intentar leer una vez si hay pending
            if self.pending_udp.is_none() {
                return;
            }
        }
        self.last_poll = Instant::now();
        let Some(pending) = self.pending_udp.clone() else { return; };
        let Some(sock) = &self.udp_socket else { return; };
        let mut buf = [0u8; 2048];
        match sock.recv_from(&mut buf) {
            Ok((n, _from)) => {
                let payload = &buf[..n];
                let frame = build_udp_ipv4_frame(
                    self.gateway_mac,
                    self.guest_mac,
                    pending.dst_ip,
                    self.guest_ip,
                    pending.dst_port,
                    pending.guest_port,
                    payload,
                );
                self.rx_queue.push_back(frame);
                self.pending_udp = None;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                if pending.started.elapsed() > Duration::from_secs(2) {
                    self.pending_udp = None;
                }
            }
            Err(_) => {
                self.pending_udp = None;
            }
        }
    }

    fn handle_arp(&mut self, frame: &[u8]) -> Result<(), NetworkError> {
        if frame.len() < 42 { return Ok(()); }
        let op = u16::from_be_bytes([frame[20], frame[21]]);
        if op != ARP_OP_REQUEST { return Ok(()); }
        let target_ip = Ipv4Addr::new(frame[38], frame[39], frame[40], frame[41]);
        if target_ip != self.gateway_ip && target_ip != self.dns_ip {
            return Ok(());
        }
        let sender_mac: [u8; 6] = frame[6..12].try_into().unwrap();
        let sender_ip = Ipv4Addr::new(frame[28], frame[29], frame[30], frame[31]);
        let mut reply = vec![0u8; 42];
        reply[0..6].copy_from_slice(&sender_mac);
        reply[6..12].copy_from_slice(&self.gateway_mac);
        reply[12..14].copy_from_slice(&ETH_TYPE_ARP.to_be_bytes());
        // HTYPE, PTYPE, HLEN, PLEN
        reply[14..16].copy_from_slice(&1u16.to_be_bytes());
        reply[16..18].copy_from_slice(&ETH_TYPE_IPV4.to_be_bytes());
        reply[18] = 6;
        reply[19] = 4;
        reply[20..22].copy_from_slice(&ARP_OP_REPLY.to_be_bytes());
        reply[22..28].copy_from_slice(&self.gateway_mac);
        reply[28..32].copy_from_slice(&target_ip.octets());
        reply[32..38].copy_from_slice(&sender_mac);
        reply[38..42].copy_from_slice(&sender_ip.octets());
        self.rx_queue.push_back(reply);
        Ok(())
    }

    fn handle_ipv4(&mut self, frame: &[u8]) -> Result<(), NetworkError> {
        if frame.len() < 34 { return Ok(()); }
        let ihl = (frame[14] & 0x0F) as usize * 4;
        if ihl < 20 || frame.len() < 14 + ihl { return Ok(()); }
        let proto = frame[23];
        let src_ip = Ipv4Addr::new(frame[26], frame[27], frame[28], frame[29]);
        let dst_ip = Ipv4Addr::new(frame[30], frame[31], frame[32], frame[33]);
        let payload = &frame[14 + ihl..];
        match proto {
            IP_PROTO_ICMP => self.handle_icmp(src_ip, dst_ip, payload),
            IP_PROTO_UDP => self.handle_udp(src_ip, dst_ip, payload),
            _ => Ok(()),
        }
    }

    fn handle_icmp(&mut self, src_ip: Ipv4Addr, dst_ip: Ipv4Addr, payload: &[u8]) -> Result<(), NetworkError> {
        if payload.len() < 8 { return Ok(()); }
        let typ = payload[0];
        let code = payload[1];
        if typ != 8 || code != 0 { return Ok(()); } // echo request
        if dst_ip != self.gateway_ip && dst_ip != self.dns_ip && dst_ip != self.guest_ip {
            return Ok(());
        }
        let mut icmp_reply = payload.to_vec();
        icmp_reply[0] = 0; // echo reply
        icmp_reply[1] = 0;
        icmp_reply[2] = 0;
        icmp_reply[3] = 0;
        let csum = internet_checksum(&icmp_reply);
        icmp_reply[2..4].copy_from_slice(&csum.to_be_bytes());
        let frame = build_ipv4_frame(self.gateway_mac, self.guest_mac, dst_ip, src_ip, IP_PROTO_ICMP, &icmp_reply);
        self.rx_queue.push_back(frame);
        Ok(())
    }

    fn handle_udp(&mut self, src_ip: Ipv4Addr, dst_ip: Ipv4Addr, payload: &[u8]) -> Result<(), NetworkError> {
        if payload.len() < 8 { return Ok(()); }
        let src_port = u16::from_be_bytes([payload[0], payload[1]]);
        let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
        let udp_len = u16::from_be_bytes([payload[4], payload[5]]) as usize;
        if udp_len < 8 || udp_len > payload.len() { return Ok(()); }
        let data = &payload[8..udp_len];

        if dst_port == DHCP_SERVER_PORT {
            self.handle_dhcp(src_port, data);
            return Ok(());
        }

        // Solo un flujo UDP pendiente a la vez (suficiente para validación M8)
        if self.pending_udp.is_some() {
            return Ok(());
        }
        let Some(sock) = &self.udp_socket else { return Ok(()); };
        let target_ip = if dst_ip == self.gateway_ip || dst_ip == self.dns_ip {
            // resolver simple: DNS/gateway virtual -> DNS público de respaldo
            Ipv4Addr::new(8, 8, 8, 8)
        } else {
            dst_ip
        };
        let _ = sock.send_to(data, SocketAddrV4::new(target_ip, dst_port));
        self.pending_udp = Some(PendingUdp {
            guest_port: src_port,
            dst_ip,
            dst_port,
            started: Instant::now(),
        });
        let _ = src_ip; // reservado para NAT completa
        Ok(())
    }

    fn handle_dhcp(&mut self, client_port: u16, data: &[u8]) {
        if data.len() < 240 { return; }
        let op = data[0];
        if op != 1 { return; } // BOOTREQUEST
        let xid = &data[4..8];
        let chaddr = &data[28..44];
        let msg_type = dhcp_message_type(data).unwrap_or(1);
        match msg_type {
            1 => { // DISCOVER -> OFFER
                if let Some(frame) = self.build_dhcp_reply(xid, chaddr, 2, client_port) {
                    self.dhcp_offered = true;
                    self.rx_queue.push_back(frame);
                }
            }
            3 => { // REQUEST -> ACK
                if let Some(frame) = self.build_dhcp_reply(xid, chaddr, 5, client_port) {
                    self.dhcp_acked = true;
                    self.rx_queue.push_back(frame);
                }
            }
            _ => {}
        }
    }

    fn build_dhcp_reply(&self, xid: &[u8], chaddr: &[u8], msg_type: u8, client_port: u16) -> Option<Vec<u8>> {
        let mut dhcp = vec![0u8; 300];
        dhcp[0] = 2; // BOOTREPLY
        dhcp[1] = 1; // Ethernet
        dhcp[2] = 6; // MAC len
        dhcp[4..8].copy_from_slice(xid);
        dhcp[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); // broadcast flag
        dhcp[16..20].copy_from_slice(&self.guest_ip.octets()); // yiaddr
        dhcp[20..24].copy_from_slice(&self.gateway_ip.octets()); // siaddr
        dhcp[28..44].copy_from_slice(chaddr);
        // magic cookie
        dhcp[236..240].copy_from_slice(&[99, 130, 83, 99]);
        let mut i = 240;
        // DHCP message type
        dhcp[i..i+3].copy_from_slice(&[53, 1, msg_type]); i += 3;
        // server identifier
        dhcp[i..i+6].copy_from_slice(&[54, 4, self.gateway_ip.octets()[0], self.gateway_ip.octets()[1], self.gateway_ip.octets()[2], self.gateway_ip.octets()[3]]); i += 6;
        // lease time
        dhcp[i..i+6].copy_from_slice(&[51, 4, 0, 0, 0x1C, 0x20]); i += 6;
        // subnet mask
        dhcp[i..i+6].copy_from_slice(&[1, 4, self.netmask.octets()[0], self.netmask.octets()[1], self.netmask.octets()[2], self.netmask.octets()[3]]); i += 6;
        // router
        dhcp[i..i+6].copy_from_slice(&[3, 4, self.gateway_ip.octets()[0], self.gateway_ip.octets()[1], self.gateway_ip.octets()[2], self.gateway_ip.octets()[3]]); i += 6;
        // DNS
        dhcp[i..i+6].copy_from_slice(&[6, 4, self.dns_ip.octets()[0], self.dns_ip.octets()[1], self.dns_ip.octets()[2], self.dns_ip.octets()[3]]); i += 6;
        // end
        dhcp[i] = 255; i += 1;
        dhcp.truncate(i);

        let frame = build_udp_ipv4_frame(
            self.gateway_mac,
            [0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            self.gateway_ip,
            Ipv4Addr::new(255, 255, 255, 255),
            DHCP_SERVER_PORT,
            if client_port == 0 { DHCP_CLIENT_PORT } else { client_port },
            &dhcp,
        );
        Some(frame)
    }
}

fn dhcp_message_type(data: &[u8]) -> Option<u8> {
    if data.len() < 240 { return None; }
    if &data[236..240] != [99, 130, 83, 99] { return None; }
    let mut i = 240;
    while i < data.len() {
        match data[i] {
            255 => break,
            0 => { i += 1; }
            opt => {
                if i + 1 >= data.len() { break; }
                let len = data[i + 1] as usize;
                if i + 2 + len > data.len() { break; }
                if opt == 53 && len == 1 { return Some(data[i + 2]); }
                i += 2 + len;
            }
        }
    }
    None
}

fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for ch in &mut chunks {
        sum += u16::from_be_bytes([ch[0], ch[1]]) as u32;
    }
    if let Some(&rem) = chunks.remainder().first() {
        sum += (rem as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn build_ipv4_frame(src_mac: [u8;6], dst_mac: [u8;6], src_ip: Ipv4Addr, dst_ip: Ipv4Addr, proto: u8, payload: &[u8]) -> Vec<u8> {
    let total_len = 20 + payload.len();
    let mut frame = vec![0u8; 14 + total_len];
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&src_mac);
    frame[12..14].copy_from_slice(&ETH_TYPE_IPV4.to_be_bytes());
    let ip = &mut frame[14..];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    ip[8] = 64; // TTL
    ip[9] = proto;
    ip[12..16].copy_from_slice(&src_ip.octets());
    ip[16..20].copy_from_slice(&dst_ip.octets());
    let csum = internet_checksum(&ip[..20]);
    ip[10..12].copy_from_slice(&csum.to_be_bytes());
    ip[20..].copy_from_slice(payload);
    frame
}

fn build_udp_ipv4_frame(src_mac: [u8;6], dst_mac: [u8;6], src_ip: Ipv4Addr, dst_ip: Ipv4Addr, src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let mut udp = vec![0u8; udp_len];
    udp[0..2].copy_from_slice(&src_port.to_be_bytes());
    udp[2..4].copy_from_slice(&dst_port.to_be_bytes());
    udp[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    udp[8..].copy_from_slice(payload);
    // UDP checksum opcional en IPv4: dejar 0
    build_ipv4_frame(src_mac, dst_mac, src_ip, dst_ip, IP_PROTO_UDP, &udp)
}
