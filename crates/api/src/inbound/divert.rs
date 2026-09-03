use dynet_core::{Error, Instance};
use smoltcp::phy::{Device, Medium, RxToken, TunTapInterface, TxToken, wait};
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{IpAddress, IpProtocol, Ipv4Packet, TcpPacket};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SEAT: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 1);
const SPARE: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 129);
const FIRST: u16 = 20000;
const REST: Duration = Duration::from_millis(5);
const BRIEF: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug)]
struct Held {
    caller: SocketAddrV4,
    target: SocketAddrV4,
}

pub struct Divert<'a> {
    seat: u16,
    mark: u32,
    book: Mutex<HashMap<u16, Held>>,
    next: AtomicU16,
    carried: AtomicUsize,
    seen: AtomicUsize,
    told: &'a (dyn Fn(&str) + Sync),
}

pub fn divert(
    instance: &Instance,
    span: Duration,
    told: &(dyn Fn(&str) + Sync),
) -> Result<usize, Error> {
    let listener = TcpListener::bind((SEAT, 0))
        .map_err(|error| Error::new(format!("cannot seat the diverter: {error}")))?;
    let seat = listener
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the diverter seat: {error}")))?
        .port();
    told(&format!("diverting to {SEAT}:{seat}"));
    let divert = Divert {
        seat,
        mark: instance.mark(),
        book: Mutex::new(HashMap::new()),
        next: AtomicU16::new(FIRST),
        carried: AtomicUsize::new(0),
        seen: AtomicUsize::new(0),
        told,
    };
    std::thread::scope(|scope| {
        scope.spawn(|| divert.greet(&listener));
        divert.pump(instance, span)
    })?;
    Ok(divert.carried.load(Ordering::Relaxed))
}

impl Divert<'_> {
    fn pump(&self, instance: &Instance, span: Duration) -> Result<(), Error> {
        let mut device = TunTapInterface::new(instance.get(), Medium::Ip)
            .map_err(|error| Error::new(format!("cannot open the device: {error}")))?;
        let started = Instant::now();
        while started.elapsed() < span {
            let beat =
                Beat::from_millis(i64::try_from(started.elapsed().as_millis()).unwrap_or_default());
            let held = device.receive(beat);
            let Some((taken, sending)) = held else {
                let _ = wait(device.as_raw_fd(), Some(REST.into()));
                continue;
            };
            let mut carried = taken.consume(<[u8]>::to_vec);
            self.count(&carried);
            if self.turn(&mut carried) {
                sending.consume(carried.len(), |room| room.copy_from_slice(&carried));
            }
        }
        Ok(())
    }

    fn count(&self, body: &[u8]) {
        let held = self.seen.fetch_add(1, Ordering::Relaxed);
        if held > 6 {
            return;
        }
        let Ok(packet) = Ipv4Packet::new_checked(body) else {
            (self.told)(&format!(
                "packet {held} did not parse, {} bytes",
                body.len()
            ));
            return;
        };
        (self.told)(&format!(
            "packet {held} {:?} {} to {}",
            packet.next_header(),
            packet.src_addr(),
            packet.dst_addr()
        ));
    }

    fn turn(&self, body: &mut [u8]) -> bool {
        let Ok(packet) = Ipv4Packet::new_checked(&*body) else {
            return false;
        };
        if packet.next_header() != IpProtocol::Tcp {
            return false;
        }
        let source = packet.src_addr();
        let target = packet.dst_addr();
        let Some(ports) = ends(&packet) else {
            return false;
        };
        match target == SPARE {
            true => self.back(body, ports),
            false => self.forth(body, (source, target), ports),
        }
    }

    fn forth(&self, body: &mut [u8], seen: (Ipv4Addr, Ipv4Addr), ports: (u16, u16)) -> bool {
        let held = Held {
            caller: SocketAddrV4::new(seen.0, ports.0),
            target: SocketAddrV4::new(seen.1, ports.1),
        };
        let spare = self.claim(held);
        write(body, (SPARE, SEAT), (spare, self.seat));
        true
    }

    fn back(&self, body: &mut [u8], ports: (u16, u16)) -> bool {
        let Some(held) = self.recall(ports.1) else {
            return false;
        };
        write(
            body,
            (*held.target.ip(), *held.caller.ip()),
            (held.target.port(), held.caller.port()),
        );
        true
    }

    fn claim(&self, held: Held) -> u16 {
        if let Ok(book) = self.book.lock() {
            for (spare, kept) in book.iter() {
                if kept.caller == held.caller && kept.target == held.target {
                    return *spare;
                }
            }
        }
        let spare = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut book) = self.book.lock() {
            book.insert(spare, held);
        }
        (self.told)(&format!(
            "{} to {} becomes {SPARE}:{spare}",
            held.caller, held.target
        ));
        spare
    }

    fn greet(&self, listener: &TcpListener) {
        for held in listener.incoming() {
            let Ok(stream) = held else {
                continue;
            };
            std::thread::scope(|scope| {
                scope.spawn(|| self.serve(stream));
            });
        }
    }

    fn serve(&self, stream: TcpStream) {
        let Ok(peer) = stream.peer_addr() else {
            return;
        };
        let Some(held) = self.recall(peer.port()) else {
            (self.told)(&format!("no session for {peer}"));
            return;
        };
        (self.told)(&format!(
            "{} asked for {}, recovered from the table",
            held.caller, held.target
        ));
        match crate::outbound::reach(held.target.into(), self.mark, BRIEF) {
            Ok(far) => self.carry(stream, far, held),
            Err(error) => (self.told)(&format!("cannot reach {}: {error}", held.target)),
        }
    }

    fn recall(&self, spare: u16) -> Option<Held> {
        self.book.lock().ok()?.get(&spare).copied()
    }

    fn carry(&self, near: TcpStream, far: TcpStream, held: Held) {
        let Ok(back) = far.try_clone() else {
            return;
        };
        let Ok(again) = near.try_clone() else {
            return;
        };
        std::thread::scope(|scope| {
            scope.spawn(|| shuttle(near, far));
            shuttle(back, again);
        });
        self.carried.fetch_add(1, Ordering::Relaxed);
        (self.told)(&format!("{} carried to {}", held.caller, held.target));
    }
}

fn shuttle(mut from: TcpStream, mut into: TcpStream) {
    let mut room = [0u8; 16384];
    while let Ok(size) = from.read(&mut room) {
        if size == 0 || into.write_all(&room[..size]).is_err() {
            break;
        }
    }
    let _ = into.shutdown(std::net::Shutdown::Write);
}

fn ends(packet: &Ipv4Packet<&[u8]>) -> Option<(u16, u16)> {
    let segment = TcpPacket::new_checked(packet.payload()).ok()?;
    Some((segment.src_port(), segment.dst_port()))
}

fn write(body: &mut [u8], seats: (Ipv4Addr, Ipv4Addr), ports: (u16, u16)) {
    let source = seats.0;
    let target = seats.1;
    let mut packet = Ipv4Packet::new_unchecked(body);
    packet.set_src_addr(source);
    packet.set_dst_addr(target);
    let mut segment = TcpPacket::new_unchecked(packet.payload_mut());
    segment.set_src_port(ports.0);
    segment.set_dst_port(ports.1);
    segment.fill_checksum(&IpAddress::Ipv4(source), &IpAddress::Ipv4(target));
    packet.fill_checksum();
}
