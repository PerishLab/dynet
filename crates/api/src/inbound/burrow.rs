use super::link::{Link, Taken};
use super::pump::Served;
use super::warren::{self, Charge, Pools, Warren};
use dynet_core::{Bearing, Ground, Name, Verdict};
use smoltcp::iface::{SocketHandle as Seat, SocketSet};
use smoltcp::socket::udp;
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const ROOM: usize = 2097152;
const DEPTH: usize = 1024;
const IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Tuple {
    peer: IpEndpoint,
    seat: IpEndpoint,
}

struct Bolt {
    link: Link,
    charge: Charge,
    opened: Instant,
    touched: Instant,
    settled: bool,
    spent: bool,
}

pub struct Burrow<'a> {
    seats: HashMap<u16, Seat>,
    bolts: HashMap<Tuple, Bolt>,
    pools: &'a Pools,
    pub served: Served,
}

impl Bolt {
    fn new(link: Link, charge: Charge) -> Self {
        Self {
            link,
            charge,
            opened: Instant::now(),
            touched: Instant::now(),
            settled: false,
            spent: false,
        }
    }
}

impl<'a> Burrow<'a> {
    pub fn new(pools: &'a Pools) -> Self {
        Self {
            seats: HashMap::new(),
            bolts: HashMap::new(),
            pools,
            served: Served::default(),
        }
    }

    pub fn hold(&mut self, sockets: &mut SocketSet<'a>, ports: &[u16]) {
        for port in ports {
            if self.seats.contains_key(port) {
                continue;
            }
            let mut socket = udp::Socket::new(room(), room());
            let listening = IpListenEndpoint {
                addr: None,
                port: *port,
            };
            if socket.bind(listening).is_err() {
                continue;
            }
            self.seats.insert(*port, sockets.add(socket));
        }
    }

    pub fn drain(&mut self, warren: &Warren, sockets: &mut SocketSet) {
        for seat in self.seats.values().copied().collect::<Vec<Seat>>() {
            while let Some((tuple, body)) = taken(sockets, seat) {
                self.deliver(warren, tuple, &body);
            }
        }
    }

    pub fn spill(&mut self, warren: &Warren, sockets: &mut SocketSet) {
        for tuple in self.bolts.keys().copied().collect::<Vec<Tuple>>() {
            self.pour(warren, tuple, sockets);
        }
    }

    pub fn reap(&mut self, warren: &Warren) {
        let now = Instant::now();
        let gone: Vec<Tuple> = self
            .bolts
            .iter()
            .filter(|(_, bolt)| bolt.spent || now > bolt.touched + IDLE)
            .map(|(tuple, _)| *tuple)
            .collect();
        for tuple in gone {
            self.close(warren, tuple);
        }
    }

    fn close(&mut self, warren: &Warren, tuple: Tuple) {
        if let Some(bolt) = self.bolts.get(&tuple).filter(|held| !held.settled) {
            let spent = bolt.opened.elapsed().as_millis();
            (warren.told)(&format!("{} gone after {spent}ms unanswered", tuple.peer));
        }
        self.bolts.remove(&tuple);
        self.served.reaped += 1;
    }

    fn deliver(&mut self, warren: &Warren, tuple: Tuple, body: &[u8]) {
        if let Some(bolt) = self.bolts.get_mut(&tuple) {
            bolt.touched = Instant::now();
            bolt.link.offer(body);
            return;
        }
        self.dig(warren, tuple, body);
    }

    fn dig(&mut self, warren: &Warren, tuple: Tuple, body: &[u8]) {
        let Some(wanted) = self.decide(warren, tuple) else {
            return;
        };
        let Some((link, charge)) = self.reach(warren, &wanted, tuple) else {
            return;
        };
        let mut bolt = Bolt::new(link, charge);
        bolt.link.offer(body);
        self.bolts.insert(tuple, bolt);
    }

    fn decide(&mut self, warren: &Warren, tuple: Tuple) -> Option<Name> {
        let router = warren.router.lock().ok()?;
        let decision = router.reached(tuple.seat.addr.into(), Instant::now());
        drop(router);
        if decision.ground() == Ground::Named {
            self.served.named += 1;
        }
        Some(decision.cluster().clone())
    }

    fn reach(&mut self, warren: &Warren, wanted: &Name, tuple: Tuple) -> Option<(Link, Charge)> {
        if !warren.bearing(wanted) {
            self.refuse(warren, wanted, "declares no datagram carriage");
            return None;
        }
        let Some(chosen) = warren.passage(self.pools, wanted, Bearing::Datagram) else {
            self.refuse(warren, wanted, "offered no node that carries them");
            return None;
        };
        if !chosen.passage.bearing() {
            self.refuse(warren, wanted, "is reached through a detour");
            return None;
        }
        (warren.told)(&format!(
            "{} to {} through {}",
            tuple.peer, tuple.seat, chosen.told
        ));
        self.served.accepted += 1;
        let seat = tuple.seat;
        let link = Link::bear(chosen.passage, seat.addr.to_string(), seat.port);
        Some((link, chosen.charge))
    }

    fn refuse(&mut self, warren: &Warren, wanted: &Name, why: &str) {
        (warren.told)(&format!("dropped: cluster {} {why}", wanted.get()));
        self.served.refused += 1;
    }

    fn pour(&mut self, warren: &Warren, tuple: Tuple, sockets: &mut SocketSet) {
        let Some(seat) = self.seats.get(&tuple.seat.port).copied() else {
            return;
        };
        let Some(bolt) = self.bolts.get_mut(&tuple) else {
            return;
        };
        let verdict = bolt.link.verdict();
        let mut borne = false;
        match bolt.link.take() {
            Taken::Body(body) => {
                bolt.touched = Instant::now();
                borne = true;
                spoken(sockets, seat, tuple, &body);
            }
            Taken::Spent => bolt.spent = true,
            Taken::Empty => {}
        }
        self.judge(warren, tuple, verdict);
        if borne {
            self.answer(warren, tuple);
        }
    }

    fn answer(&mut self, warren: &Warren, tuple: Tuple) {
        let Some(bolt) = self.bolts.get_mut(&tuple) else {
            return;
        };
        if bolt.settled {
            return;
        }
        bolt.settled = true;
        let spent = bolt.opened.elapsed().as_millis();
        let charge = bolt.charge.clone();
        warren::observed(self.pools, &charge, Verdict::Answered);
        (warren.told)(&format!("{} answered after {spent}ms", tuple.peer));
        self.served.answered += 1;
    }

    fn judge(&mut self, warren: &Warren, tuple: Tuple, verdict: Option<Verdict>) {
        let Some(verdict) = verdict else {
            return;
        };
        let Some(bolt) = self.bolts.get_mut(&tuple) else {
            return;
        };
        if bolt.settled {
            return;
        }
        bolt.settled = true;
        let charge = bolt.charge.clone();
        warren::observed(self.pools, &charge, verdict);
        (warren.told)(&format!("{} verdict {verdict:?}", tuple.peer));
        match matches!(verdict, Verdict::Faulted(_)) {
            true => self.served.faulted += 1,
            false => self.served.answered += 1,
        }
    }
}

fn room() -> udp::PacketBuffer<'static> {
    udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; DEPTH], vec![0; ROOM])
}

fn taken(sockets: &mut SocketSet, seat: Seat) -> Option<(Tuple, Vec<u8>)> {
    let socket = sockets.get_mut::<udp::Socket>(seat);
    let port = socket.endpoint().port;
    let (body, meta) = socket.recv().ok()?;
    let held = meta.local_address?;
    let tuple = Tuple {
        peer: meta.endpoint,
        seat: IpEndpoint::new(held, port),
    };
    Some((tuple, body.to_vec()))
}

fn spoken(sockets: &mut SocketSet, seat: Seat, tuple: Tuple, body: &[u8]) {
    let socket = sockets.get_mut::<udp::Socket>(seat);
    let mut meta = udp::UdpMetadata::from(tuple.peer);
    meta.local_address = Some(tuple.seat.addr);
    let _ = socket.send_slice(body, meta);
}
