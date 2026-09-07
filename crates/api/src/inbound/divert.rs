mod peek;
mod roll;
mod usher;

use super::lasting;
use super::warren::{Pools, Served, Warren, gather, renew, stand};
use dynet_core::{Error, Span};
pub(super) use roll::Held;
use roll::Roll;
use smoltcp::phy::{Device, Medium, RxToken, TunTapInterface, TxToken, wait};
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{IpAddress, IpProtocol, Ipv4Packet, TcpPacket, UdpPacket};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, UdpSocket};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(5);
const SWEEP: Duration = Duration::from_secs(30);
const CROWD: usize = 256;

pub struct Divert<'a> {
    pub(super) span: Span,
    pub(super) seat: u16,
    pub(super) dart: u16,
    book: Mutex<Roll>,
    full: Once,
    done: AtomicBool,
    pub(super) warren: &'a Warren<'a>,
    pub(super) pools: &'a Pools,
    pub(super) served: Mutex<Served>,
    pub(super) shy: Mutex<std::collections::HashMap<SocketAddrV4, usize>>,
}

pub fn divert(warren: &Warren, ground: (Span, Duration, u16)) -> Result<Served, Error> {
    let (span, patience, transit) = ground;
    let listener = TcpListener::bind((span.seat(), 0))
        .map_err(|error| Error::new(format!("cannot seat the diverter: {error}")))?;
    let seat = listener
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the diverter seat: {error}")))?
        .port();
    let darts = UdpSocket::bind((span.seat(), 0))
        .map_err(|error| Error::new(format!("cannot seat the datagram diverter: {error}")))?;
    let dart = darts
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read the datagram seat: {error}")))?
        .port();
    (warren.told)(&format!("diverting to {}:{seat} and :{dart}", span.seat()));
    let pools = gather(warren, Instant::now())?;
    let divert = Divert {
        span,
        seat,
        dart,
        book: Mutex::new(Roll::new()),
        full: Once::new(),
        done: AtomicBool::new(false),
        warren,
        pools: &pools,
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
        scope.spawn(|| stand(warren, &pools, (span.seat(), patience)));
        scope.spawn(|| divert.greet(&listener));
        scope.spawn(|| divert.flock(&darts));
        if let Some(seat) = &ushered {
            scope.spawn(|| divert.usher(seat));
        }
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
        let mut swept = Duration::ZERO;
        while lasting(started, span) {
            let since = started.elapsed();
            if since >= swept + SWEEP {
                swept = since;
                self.glean(started);
            }
            if self.warren.reload.swap(false, Ordering::Relaxed) {
                renew(self.warren);
            }
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
        self.done.store(true, Ordering::Relaxed);
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
        self.warren.forget();
        super::standing::record(self.warren, self.pools, started);
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
        drop(book);
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
