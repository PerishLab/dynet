use super::IDLE;
use super::divert::Divert;
use super::divert::Held;
use super::link::{Link, Taken};
use super::warren::{self, Charge, Chosen};
use dynet_core::{Bearing, Ground, Verdict};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TrySendError;
use std::time::{Duration, Instant};

const ROOM: usize = 65536;
const REST: Duration = Duration::from_millis(5);

enum Fed {
    Away,
    Full,
    Gone,
}

struct Perch {
    sending: SyncSender<Vec<u8>>,
    touched: Instant,
}

type Flock = Mutex<HashMap<u16, Perch>>;

impl Divert<'_> {
    pub(super) fn greet(&self, listener: &TcpListener) {
        let _ = listener.set_nonblocking(true);
        std::thread::scope(|scope| {
            while !self.spent() {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(REST);
                    continue;
                };
                let _ = stream.set_nonblocking(false);
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
        self.tend(stream, held);
        self.end(peer.port());
    }

    pub(super) fn tend(&self, stream: TcpStream, held: Held) {
        let Some(chosen) = self.elect(&stream, held.target) else {
            self.tally(|served| served.refused += 1);
            return;
        };
        self.opened(held.target, &chosen);
        let link = Link::open(
            chosen.passage,
            held.target.ip().to_string(),
            held.target.port(),
        );
        self.carry(stream, link, &chosen.charge);
    }

    pub(super) fn choose(&self, target: SocketAddrV4, bearing: Bearing) -> Option<Chosen> {
        let router = self.warren.router.lock().ok()?;
        let decision = router.reached(IpAddr::V4(*target.ip()), Instant::now());
        drop(router);
        if decision.ground() == Ground::Named {
            self.tally(|served| served.named += 1);
        }
        if bearing == Bearing::Datagram && !self.warren.bearing(decision.cluster()) {
            return None;
        }
        self.warren.passage(self.pools, decision.cluster(), bearing)
    }

    pub(super) fn opened(&self, target: SocketAddrV4, chosen: &Chosen) {
        (self.told())(&format!("{target} through {}", chosen.told));
        self.tally(|served| served.accepted += 1);
    }

    pub(super) fn judge(&self, link: &Link, charge: &Charge) {
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

    fn carry(&self, near: TcpStream, mut link: Link, charge: &Charge) {
        let Ok(again) = near.try_clone() else {
            return;
        };
        let sending = link.upward();
        std::thread::scope(|scope| {
            scope.spawn(|| push(again, sending));
            drain(near, &link);
        });
        self.judge(&link, charge);
    }

    pub(super) fn flock(&self, darts: &UdpSocket) {
        let _ = darts.set_read_timeout(Some(REST));
        let flock: Flock = Mutex::new(HashMap::new());
        let held = &flock;
        let mut room = [0u8; ROOM];
        std::thread::scope(|scope| {
            while !self.spent() {
                let Ok((size, peer)) = darts.recv_from(&mut room) else {
                    continue;
                };
                let body = &room[..size];
                if self.fed(held, peer.port(), body) {
                    continue;
                }
                let Some((link, charge)) = self.open(peer, held) else {
                    self.end(peer.port());
                    continue;
                };
                feed(held, peer.port(), body);
                scope.spawn(move || self.spill(link, charge, (darts, peer, held)));
            }
        });
    }

    fn fed(&self, held: &Flock, port: u16, body: &[u8]) -> bool {
        match feed(held, port, body) {
            Fed::Away => true,
            Fed::Full => {
                self.tally(|served| served.dropped += 1);
                true
            }
            Fed::Gone => false,
        }
    }

    fn open(&self, peer: SocketAddr, held: &Flock) -> Option<(Link, Charge)> {
        let kept = self.recall(peer.port())?;
        let Some(chosen) = self.choose(kept.target, Bearing::Datagram) else {
            self.tally(|served| served.refused += 1);
            return None;
        };
        self.opened(kept.target, &chosen);
        let mut link = Link::bear(
            chosen.passage,
            kept.target.ip().to_string(),
            kept.target.port(),
        );
        let sending = link.upward()?;
        let perch = Perch {
            sending,
            touched: Instant::now(),
        };
        held.lock().ok()?.insert(peer.port(), perch);
        Some((link, chosen.charge))
    }

    fn spill(&self, link: Link, charge: Charge, seat: (&UdpSocket, SocketAddr, &Flock)) {
        let (_, peer, held) = seat;
        let mut borne = false;
        let mut patience = IDLE;
        while let Some(next) = self.tick(&link, patience, seat, (&charge, &mut borne)) {
            patience = next;
        }
        if !borne {
            self.judge(&link, &charge);
        }
        if let Ok(mut kept) = held.lock() {
            kept.remove(&peer.port());
        }
        self.end(peer.port());
    }

    fn tick(
        &self,
        link: &Link,
        patience: Duration,
        seat: (&UdpSocket, SocketAddr, &Flock),
        told: (&Charge, &mut bool),
    ) -> Option<Duration> {
        let (darts, peer, held) = seat;
        let (charge, borne) = told;
        match link.bide(patience) {
            Taken::Body(body) => self.pour(&body, (darts, peer), charge, borne).then(|| {
                touch(held, peer.port());
                IDLE
            }),
            Taken::Spent => None,
            Taken::Empty => self.rest(held, peer),
        }
    }

    fn rest(&self, held: &Flock, peer: SocketAddr) -> Option<Duration> {
        let Some(left) = shed(held, peer.port()) else {
            (self.told())(&format!("{peer} dropped after {}s quiet", IDLE.as_secs()));
            return None;
        };
        Some(left)
    }

    fn pour(
        &self,
        body: &[u8],
        seat: (&UdpSocket, SocketAddr),
        charge: &Charge,
        borne: &mut bool,
    ) -> bool {
        if !*borne {
            *borne = true;
            self.answer(charge);
        }
        seat.0.send_to(body, seat.1).is_ok()
    }

    fn answer(&self, charge: &Charge) {
        warren::observed(self.pools, charge, Verdict::Answered);
        (self.told())("answered");
        self.tally(|served| served.answered += 1);
    }
}

fn feed(held: &Flock, port: u16, body: &[u8]) -> Fed {
    let Ok(mut kept) = held.lock() else {
        return Fed::Gone;
    };
    let Some(perch) = kept.get_mut(&port) else {
        return Fed::Gone;
    };
    perch.touched = Instant::now();
    match perch.sending.try_send(body.to_vec()) {
        Ok(()) => Fed::Away,
        Err(TrySendError::Full(_)) => Fed::Full,
        Err(TrySendError::Disconnected(_)) => Fed::Gone,
    }
}

fn touch(held: &Flock, port: u16) {
    let Ok(mut kept) = held.lock() else {
        return;
    };
    let Some(perch) = kept.get_mut(&port) else {
        return;
    };
    perch.touched = Instant::now();
}

fn shed(held: &Flock, port: u16) -> Option<Duration> {
    let mut kept = held.lock().ok()?;
    let spent = kept.get(&port)?.touched.elapsed();
    if spent >= IDLE {
        kept.remove(&port);
        return None;
    }
    Some(IDLE - spent)
}

fn push(mut from: TcpStream, sending: Option<SyncSender<Vec<u8>>>) {
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
