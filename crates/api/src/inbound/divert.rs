mod peek;
mod roll;
mod route;
mod stage;
mod stream;
mod usher;

use super::lasting;
use super::warren::{Served, Warren, stand};
use dynet_core::{Error, Span};
use dynet_host::{Device, ROOM};
pub(super) use roll::Held;
use roll::Roll;
use smoltcp::wire::{IpAddress, IpProtocol, Ipv4Packet, TcpPacket, UdpPacket};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(5);
const SWEEP: Duration = Duration::from_secs(30);
const HEED: Duration = Duration::from_secs(1);
const CROWD: usize = 256;

pub struct Divert<'a> {
    pub(super) span: Span,
    pub(super) seat: u16,
    pub(super) dart: u16,
    book: Mutex<Roll>,
    full: Once,
    done: AtomicBool,
    pub(super) warren: &'a Warren<'a>,
    pub(super) served: Mutex<Served>,
    pub(super) shy: Mutex<std::collections::HashMap<SocketAddrV4, usize>>,
}

pub fn divert(warren: &Warren, ground: (Span, Duration, u16)) -> Result<Served, Error> {
    let (span, patience, transit) = ground;
    let stage::Seats {
        device,
        listener,
        darts,
        resolver,
    } = stage::stage(warren, span)?;
    #[cfg(windows)]
    dynet_host::windows::ready();
    let seat = listener
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the diverter seat: {error}")))?
        .port();
    let dart = darts
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the datagram seat: {error}")))?
        .port();
    (warren.told)(&format!("diverting to {}:{seat} and :{dart}", span.seat()));
    let divert = Divert {
        span,
        seat,
        dart,
        book: Mutex::new(Roll::new()),
        full: Once::new(),
        done: AtomicBool::new(false),
        warren,
        served: Mutex::new(Served::default()),
        shy: Mutex::new(std::collections::HashMap::new()),
    };
    let ushered = match transit {
        0 => None,
        port => Some(usher::seat(port)?),
    };
    if let Some(seat) = &ushered {
        (warren.told)(&format!(
            "ushering what the kernel hands over on :{transit}"
        ));
        let _ = seat;
    }
    std::thread::scope(|scope| {
        scope.spawn(|| stand(warren, patience, &resolver));
        scope.spawn(|| divert.greet(&listener));
        scope.spawn(|| divert.flock(&darts));
        if let Some(seat) = &ushered {
            scope.spawn(|| divert.usher(seat, transit));
        }
        divert.pump(&device, patience)
    })?;
    divert
        .served
        .into_inner()
        .map_err(|_| Error::new("a diverted session died holding the ledger"))
}

impl Divert<'_> {
    fn pump(&self, device: &Device, span: Duration) -> Result<(), Error> {
        let carried = self.circulate(device, span);
        self.done.store(true, Ordering::Relaxed);
        self.warren.halt.store(true, Ordering::Relaxed);
        self.warren.heed();
        carried
    }

    fn circulate(&self, device: &Device, span: Duration) -> Result<(), Error> {
        let mut room = vec![0; ROOM];
        let started = Instant::now();
        let mut swept = Duration::ZERO;
        let mut heeded = Duration::ZERO;
        while lasting(started, span) && !self.warren.halted() {
            let since = started.elapsed();
            if since >= swept + SWEEP {
                swept = since;
                self.glean(started);
            }
            if since >= heeded + HEED {
                heeded = since;
                self.warren.heed();
            }
            if self.warren.reload.swap(false, Ordering::Relaxed) {
                self.warren.renew();
            }
            let held = device
                .receive(&mut room)
                .map_err(|error| Error::new(format!("cannot read the device: {error}")))?;
            let Some(size) = held else {
                let _ = device.wait(REST);
                continue;
            };
            let carried = &mut room[..size];
            if self.turn(carried) {
                let _ = device.send(carried);
            }
        }
        Ok(())
    }

    pub(super) fn spent(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    fn turn(&self, body: &mut [u8]) -> bool {
        let Ok(packet) = Ipv4Packet::new_checked(&*body) else {
            return false;
        };
        let borne = match packet.next_header() {
            IpProtocol::Tcp => false,
            IpProtocol::Udp => true,
            _ => return false,
        };
        let source = packet.src_addr();
        let target = packet.dst_addr();
        let Some(ports) = ends(&packet, borne) else {
            return false;
        };
        let told = (ports.0, ports.1, borne);
        match target == self.span.spare() {
            true => self.back(body, told),
            false => self.forth(body, (source, target), told),
        }
    }

    fn forth(&self, body: &mut [u8], seen: (Ipv4Addr, Ipv4Addr), told: (u16, u16, bool)) -> bool {
        let (source, target, borne) = told;
        let held = Held {
            caller: SocketAddrV4::new(seen.0, source),
            target: SocketAddrV4::new(seen.1, target),
        };
        let Some(spare) = self.claim(held) else {
            return false;
        };
        let seat = match borne {
            true => self.dart,
            false => self.seat,
        };
        write(
            body,
            (self.span.spare(), self.span.seat()),
            (spare, seat, borne),
        );
        true
    }

    fn back(&self, body: &mut [u8], told: (u16, u16, bool)) -> bool {
        let Some(held) = self.recall(told.1) else {
            return false;
        };
        write(
            body,
            (*held.target.ip(), *held.caller.ip()),
            (held.target.port(), held.caller.port(), told.2),
        );
        true
    }

    fn claim(&self, held: Held) -> Option<u16> {
        let taken = self.book.lock().ok()?.claim(held);
        let Some((spare, fresh)) = taken else {
            self.full.call_once(|| {
                (self.told())("the session roll is full, so new sessions are refused");
            });
            self.tally(|served| served.refused += 1);
            return None;
        };
        if fresh {
            (self.told())(&format!(
                "{} to {} becomes {}:{spare}",
                held.caller,
                held.target,
                self.span.spare()
            ));
        }
        Some(spare)
    }

    pub(super) fn end(&self, spare: u16) {
        if let Ok(mut book) = self.book.lock() {
            book.end(spare);
        }
    }

    fn glean(&self, started: Instant) {
        super::standing::record(self.warren, &self.warren.current().pools, started);
        let Ok(mut book) = self.book.lock() else {
            return;
        };
        if let Ok(mut shy) = self.shy.lock()
            && shy.len() > CROWD
        {
            shy.clear();
        }
        let gone = book.reap();
        let standing = book.standing();
        let engaged = book.engaged();
        drop(book);
        self.warren.forget(&engaged);
        if gone > 0 {
            (self.told())(&format!("released {gone} sessions, {standing} standing"));
        }
    }

    pub(super) fn tally(&self, act: impl FnOnce(&mut Served)) {
        if let Ok(mut served) = self.served.lock() {
            act(&mut served);
        }
    }

    pub(super) fn told(&self) -> &(dyn Fn(&str) + Sync) {
        self.warren.told
    }

    pub(super) fn recall(&self, spare: u16) -> Option<Held> {
        self.book.lock().ok()?.recall(spare)
    }
}

fn ends(packet: &Ipv4Packet<&[u8]>, borne: bool) -> Option<(u16, u16)> {
    if borne {
        let dart = UdpPacket::new_checked(packet.payload()).ok()?;
        return Some((dart.src_port(), dart.dst_port()));
    }
    let segment = TcpPacket::new_checked(packet.payload()).ok()?;
    Some((segment.src_port(), segment.dst_port()))
}

fn write(body: &mut [u8], seats: (Ipv4Addr, Ipv4Addr), told: (u16, u16, bool)) {
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
