use super::super::{Divert, Held};
use dynet_core::{Bearing, Course, Refusal, Sight};
use dynet_host::Device;
use smoltcp::wire::{
    Icmpv4DstUnreachable, Icmpv4Message, Icmpv4Packet, IpProtocol, Ipv4Packet, UdpPacket,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

const HEADER: usize = 20;
const PREFIX: usize = HEADER + 8;
const ROOM: usize = PREFIX + 60 + 8;
const LIMIT: usize = 16;
const WINDOW: Duration = Duration::from_secs(1);

pub(in crate::inbound::divert) struct Allowance {
    since: Instant,
    sent: usize,
}

impl Allowance {
    pub(in crate::inbound::divert) fn new(since: Instant) -> Self {
        Self { since, sent: 0 }
    }

    fn take(&mut self, now: Instant) -> bool {
        if now.duration_since(self.since) >= WINDOW {
            self.since = now;
            self.sent = 0;
        }
        if self.sent == LIMIT {
            return false;
        }
        self.sent += 1;
        true
    }
}

struct Quote<'a> {
    held: Held,
    bytes: &'a [u8],
}

impl<'a> Quote<'a> {
    fn read(body: &'a [u8]) -> Option<Self> {
        let packet = Ipv4Packet::new_checked(body).ok()?;
        if !header(&packet) || !whole(&packet) {
            return None;
        }
        Self::udp(body, &packet)
    }

    fn udp(body: &'a [u8], packet: &Ipv4Packet<&[u8]>) -> Option<Self> {
        let caller = packet.src_addr();
        let target = packet.dst_addr();
        if caller == target || !addresses(caller) || !addresses(target) {
            return None;
        }
        let udp = UdpPacket::new_checked(packet.payload()).ok()?;
        if udp.src_port() == 0
            || udp.dst_port() == 0
            || usize::from(udp.len()) != packet.payload().len()
        {
            return None;
        }
        let held = Held {
            caller: SocketAddrV4::new(caller, udp.src_port()),
            target: SocketAddrV4::new(target, udp.dst_port()),
            borne: true,
        };
        let length = usize::from(packet.header_len()) + 8;
        Some(Self {
            held,
            bytes: body.get(..length)?,
        })
    }

    fn answer<'b>(&self, room: &'b mut [u8; ROOM]) -> &'b [u8] {
        let length = PREFIX + self.bytes.len();
        let mut packet = Ipv4Packet::new_unchecked(&mut room[..length]);
        packet.set_version(4);
        packet.set_header_len(HEADER as u8);
        packet.set_total_len(length as u16);
        packet.set_next_header(IpProtocol::Icmp);
        packet.set_hop_limit(64);
        packet.set_src_addr(*self.held.target.ip());
        packet.set_dst_addr(*self.held.caller.ip());
        packet.fill_checksum();
        let mut message = Icmpv4Packet::new_unchecked(packet.payload_mut());
        message.set_msg_type(Icmpv4Message::DstUnreachable);
        message.set_msg_code(Icmpv4DstUnreachable::PortUnreachable.into());
        message.data_mut().copy_from_slice(self.bytes);
        message.fill_checksum();
        &room[..length]
    }
}

fn header(packet: &Ipv4Packet<&[u8]>) -> bool {
    packet.version() == 4
        && packet.next_header() == IpProtocol::Udp
        && usize::from(packet.header_len()) >= HEADER
}

fn whole(packet: &Ipv4Packet<&[u8]>) -> bool {
    !packet.more_frags() && packet.frag_offset() == 0 && packet.verify_checksum()
}

fn addresses(address: Ipv4Addr) -> bool {
    let [front, _, _, last] = address.octets();
    matches!(front, 1..=126 | 128..=223) && !address.is_link_local() && last != 255
}

impl Divert<'_> {
    pub(in crate::inbound::divert) fn reply(
        &self,
        device: &Device,
        body: &[u8],
        allowance: &mut Allowance,
    ) -> bool {
        let Some(quote) = Quote::read(body) else {
            return false;
        };
        let target = quote.held.target;
        if *target.ip() == self.span.spare() || *target.ip() == self.span.seat() {
            return false;
        }
        let Some(decision) = self.unborne(&quote.held) else {
            return false;
        };
        self.refused(target, &decision, Bearing::Datagram);
        self.tally(|served| served.refused += 1);
        if allowance.take(Instant::now()) {
            let _ = device.send(quote.answer(&mut [0; ROOM]));
        }
        true
    }

    fn unborne(&self, held: &Held) -> Option<dynet_core::Decision> {
        if self.book.lock().ok()?.contains(held) {
            return None;
        }
        let sight = Sight {
            told: None,
            address: IpAddr::V4(*held.target.ip()),
            bearing: Bearing::Datagram,
        };
        let decision = self.warren.inspect(&sight)?;
        let unavailable = matches!(
            decision.course(),
            Course::Direct | Course::Refused(_, Refusal::Unborne)
        );
        unavailable.then_some(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::{Allowance, LIMIT, Quote, ROOM, WINDOW, addresses};
    use smoltcp::wire::{Icmpv4Message, Icmpv4Packet, IpProtocol, Ipv4Packet, UdpPacket};
    use std::time::{Duration, Instant};

    fn asking(header: u8) -> Vec<u8> {
        let size = usize::from(header) + 12;
        let mut bytes = vec![0; size];
        if header > 20 {
            bytes[20..24].copy_from_slice(&[1, 1, 0, 0]);
        }
        let mut packet = Ipv4Packet::new_unchecked(&mut bytes);
        packet.set_version(4);
        packet.set_header_len(header);
        packet.set_total_len(size as u16);
        packet.set_src_addr([192, 168, 1, 24].into());
        packet.set_dst_addr([93, 184, 216, 34].into());
        packet.set_next_header(IpProtocol::Udp);
        packet.set_hop_limit(64);
        let mut udp = UdpPacket::new_unchecked(packet.payload_mut());
        udp.set_src_port(50000);
        udp.set_dst_port(443);
        udp.set_len(12);
        udp.payload_mut().copy_from_slice(b"tail");
        packet.fill_checksum();
        bytes
    }

    #[test]
    fn wire() {
        let input = asking(24);
        let quote = Quote::read(&input).unwrap();
        let mut room = [0; ROOM];
        let reply = quote.answer(&mut room);
        let packet = Ipv4Packet::new_checked(reply).unwrap();
        assert_eq!(packet.src_addr(), *quote.held.target.ip());
        assert_eq!(packet.dst_addr(), *quote.held.caller.ip());
        assert_eq!(usize::from(packet.total_len()), reply.len());
        assert_eq!(packet.next_header(), IpProtocol::Icmp);
        assert!(packet.verify_checksum());
        let message = Icmpv4Packet::new_checked(packet.payload()).unwrap();
        assert_eq!(message.msg_type(), Icmpv4Message::DstUnreachable);
        assert_eq!(message.msg_code(), 3);
        assert!(message.verify_checksum());
        assert_eq!(message.data(), &input[..32]);
    }

    #[test]
    fn malformed() {
        let input = asking(20);
        assert!(Quote::read(&input[..27]).is_none());
        let mut invalid = input.clone();
        invalid[0] = 0x44;
        assert!(Quote::read(&invalid).is_none());
        let mut invalid = input.clone();
        invalid[8] ^= 1;
        assert!(Quote::read(&invalid).is_none());
        let mut invalid = input;
        invalid[24..26].copy_from_slice(&8u16.to_be_bytes());
        assert!(Quote::read(&invalid).is_none());
    }

    #[test]
    fn suppressed() {
        let mut input = asking(20);
        let mut packet = Ipv4Packet::new_unchecked(&mut input);
        packet.set_more_frags(true);
        packet.fill_checksum();
        assert!(Quote::read(&input).is_none());
        let mut packet = Ipv4Packet::new_unchecked(&mut input);
        packet.set_more_frags(false);
        packet.set_frag_offset(8);
        packet.fill_checksum();
        assert!(Quote::read(&input).is_none());
        for address in [
            [0, 0, 0, 0],
            [127, 0, 0, 1],
            [169, 254, 1, 2],
            [224, 0, 0, 251],
            [255, 255, 255, 255],
            [192, 168, 1, 255],
        ] {
            assert!(!addresses(address.into()));
        }
    }

    #[test]
    fn burst() {
        let now = Instant::now();
        let mut allowance = Allowance::new(now);
        assert!((0..LIMIT).all(|_| allowance.take(now)));
        assert!((0..1000).all(|_| !allowance.take(now)));
        assert!(!allowance.take(now + WINDOW - Duration::from_millis(1)));
        assert!(allowance.take(now + WINDOW));
    }
}
