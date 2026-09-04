use super::link::Link;
use super::pump::{Served, gather, stand};
use super::warren::{self, Charge, Chosen, Pools, Warren};
use dynet_core::{Bearing, Error, Ground, Span, Verdict};
use smoltcp::phy::{Device, Medium, RxToken, TunTapInterface, TxToken, wait};
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{IpAddress, IpProtocol, Ipv4Packet, TcpPacket};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const FIRST: u16 = 20000;
const REST: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug)]
struct Held {
    caller: SocketAddrV4,
    target: SocketAddrV4,
}

pub struct Divert<'a> {
    span: Span,
    seat: u16,
    book: Mutex<HashMap<u16, Held>>,
    next: AtomicU16,
    warren: &'a Warren<'a>,
    pools: &'a Pools,
    served: Mutex<Served>,
}

pub fn divert(warren: &Warren, ground: (Span, Duration)) -> Result<Served, Error> {
    let (span, patience) = ground;
    let listener = TcpListener::bind((span.seat(), 0))
        .map_err(|error| Error::new(format!("cannot seat the diverter: {error}")))?;
    let seat = listener
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the diverter seat: {error}")))?
        .port();
    (warren.told)(&format!("diverting to {}:{seat}", span.seat()));
    let pools = gather(warren, Instant::now())?;
    let divert = Divert {
        span,
        seat,
        book: Mutex::new(HashMap::new()),
        next: AtomicU16::new(FIRST),
        warren,
        pools: &pools,
        served: Mutex::new(Served::default()),
    };
    std::thread::scope(|scope| {
        scope.spawn(|| stand(warren, &pools, patience));
        scope.spawn(|| divert.greet(&listener));
        divert.pump(patience)
    })?;
    divert
        .served
        .into_inner()
        .map_err(|_| Error::new("a diverted session died holding the ledger"))
}

impl Divert<'_> {
    fn pump(&self, span: Duration) -> Result<(), Error> {
        let mut device = TunTapInterface::new(self.warren.instance.get(), Medium::Ip)
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
            if self.turn(&mut carried) {
                sending.consume(carried.len(), |room| room.copy_from_slice(&carried));
            }
        }
        Ok(())
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
        match target == self.span.spare() {
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
        write(
            body,
            (self.span.spare(), self.span.seat()),
            (spare, self.seat),
        );
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
        (self.told())(&format!(
            "{} to {} becomes {}:{spare}",
            held.caller,
            held.target,
            self.span.spare()
        ));
        spare
    }

    fn greet(&self, listener: &TcpListener) {
        std::thread::scope(|scope| {
            for held in listener.incoming() {
                let Ok(stream) = held else {
                    continue;
                };
                scope.spawn(|| self.serve(stream));
            }
        });
    }

    fn serve(&self, stream: TcpStream) {
        let Ok(peer) = stream.peer_addr() else {
            return;
        };
        let Some(held) = self.recall(peer.port()) else {
            return;
        };
        let target = held.target;
        let Some(chosen) = self.choose(target) else {
            self.tally(|served| served.refused += 1);
            return;
        };
        (self.told())(&format!("{target} through {}", chosen.told));
        self.tally(|served| served.accepted += 1);
        let link = Link::open(chosen.passage, target.ip().to_string(), target.port());
        self.carry(stream, link, &chosen.charge);
    }

    fn choose(&self, target: SocketAddrV4) -> Option<Chosen> {
        let router = self.warren.router.lock().ok()?;
        let decision = router.reached(IpAddr::V4(*target.ip()), Instant::now());
        drop(router);
        if decision.ground() == Ground::Named {
            self.tally(|served| served.named += 1);
        }
        self.warren
            .passage(self.pools, decision.cluster(), Bearing::Stream)
    }

    fn carry(&self, near: TcpStream, mut link: Link, charge: &Charge) {
        let Ok(again) = near.try_clone() else {
            return;
        };
        let sending = link.upward();
        std::thread::scope(|scope| {
            scope.spawn(|| push(again, sending));
            drain(near, &link);
        });
        let Some(verdict) = link.verdict() else {
            return;
        };
        warren::observed(self.pools, charge, verdict);
        (self.told())(&format!("verdict {verdict:?}"));
        match matches!(verdict, Verdict::Faulted(_)) {
            true => self.tally(|served| served.faulted += 1),
            false => self.tally(|served| served.answered += 1),
        }
    }

    fn tally(&self, act: impl FnOnce(&mut Served)) {
        if let Ok(mut served) = self.served.lock() {
            act(&mut served);
        }
    }

    fn told(&self) -> &(dyn Fn(&str) + Sync) {
        self.warren.told
    }

    fn recall(&self, spare: u16) -> Option<Held> {
        self.book.lock().ok()?.get(&spare).copied()
    }
}

fn push(mut from: TcpStream, sending: Option<Sender<Vec<u8>>>) {
    let Some(sending) = sending else {
        return;
    };
    let mut room = [0u8; 16384];
    while let Ok(size) = from.read(&mut room) {
        if size == 0 || sending.send(room[..size].to_vec()).is_err() {
            break;
        }
    }
}

fn drain(mut into: TcpStream, link: &Link) {
    while let Some(body) = link.wait() {
        if into.write_all(&body).is_err() {
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
