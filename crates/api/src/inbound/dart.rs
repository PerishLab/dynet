use super::divert::Divert;
use super::link::Link;
use super::warren::{self, Charge, Chosen};
use dynet_core::{Bearing, Ground, Verdict};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Instant;

const ROOM: usize = 65536;

type Flock = Mutex<HashMap<u16, Sender<Vec<u8>>>>;

impl Divert<'_> {
    pub(super) fn greet(&self, listener: &TcpListener) {
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
        let Some(chosen) = self.choose(held.target, Bearing::Stream) else {
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
        let flock: Flock = Mutex::new(HashMap::new());
        let held = &flock;
        let mut room = [0u8; ROOM];
        std::thread::scope(|scope| {
            while let Ok((size, peer)) = darts.recv_from(&mut room) {
                let body = room[..size].to_vec();
                if let Some(sending) = held
                    .lock()
                    .ok()
                    .and_then(|kept| kept.get(&peer.port()).cloned())
                {
                    let _ = sending.send(body);
                    continue;
                }
                let Some((link, charge)) = self.open(peer, held) else {
                    continue;
                };
                let _ = held
                    .lock()
                    .ok()
                    .and_then(|kept| kept.get(&peer.port()).cloned())
                    .map(|sending| sending.send(body));
                scope.spawn(move || self.spill(link, charge, (darts, peer, held)));
            }
        });
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
        held.lock().ok()?.insert(peer.port(), sending);
        Some((link, chosen.charge))
    }

    fn spill(&self, link: Link, charge: Charge, seat: (&UdpSocket, SocketAddr, &Flock)) {
        let (darts, peer, held) = seat;
        let mut borne = false;
        while let Some(body) = link.wait() {
            if !borne {
                borne = true;
                self.answer(&charge);
            }
            if darts.send_to(&body, peer).is_err() {
                break;
            }
        }
        if !borne {
            self.judge(&link, &charge);
        }
        if let Ok(mut kept) = held.lock() {
            kept.remove(&peer.port());
        }
    }

    fn answer(&self, charge: &Charge) {
        warren::observed(self.pools, charge, Verdict::Answered);
        (self.told())("answered");
        self.tally(|served| served.answered += 1);
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
