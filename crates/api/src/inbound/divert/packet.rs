use smoltcp::wire::{IpAddress, Ipv4Packet, TcpPacket, UdpPacket};
use std::net::Ipv4Addr;

pub(super) fn ends(packet: &Ipv4Packet<&[u8]>, borne: bool) -> Option<(u16, u16)> {
    if borne {
        let dart = UdpPacket::new_checked(packet.payload()).ok()?;
        return Some((dart.src_port(), dart.dst_port()));
    }
    let segment = TcpPacket::new_checked(packet.payload()).ok()?;
    Some((segment.src_port(), segment.dst_port()))
}

pub(super) fn write(body: &mut [u8], seats: (Ipv4Addr, Ipv4Addr), told: (u16, u16, bool)) {
    let source = IpAddress::Ipv4(seats.0);
    let target = IpAddress::Ipv4(seats.1);
    let (ports, borne) = ((told.0, told.1), told.2);
    let mut packet = Ipv4Packet::new_unchecked(body);
    packet.set_src_addr(seats.0);
    packet.set_dst_addr(seats.1);
    match borne {
        true => {
            let mut dart = UdpPacket::new_unchecked(packet.payload_mut());
            dart.set_src_port(ports.0);
            dart.set_dst_port(ports.1);
            dart.fill_checksum(&source, &target);
        }
        false => {
            let mut segment = TcpPacket::new_unchecked(packet.payload_mut());
            segment.set_src_port(ports.0);
            segment.set_dst_port(ports.1);
            segment.fill_checksum(&source, &target);
        }
    }
    packet.fill_checksum();
}
