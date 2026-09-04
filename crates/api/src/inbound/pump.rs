use super::burrow::Burrow;
use super::clock::{beat, lasting, raise};
use super::link::Link;
use super::strand::{Strand, downward, upward};
use super::warden;
use super::warren::{self, Pools, Warren};
use dynet_core::{Bearing, Error, Ground, Name, Policy, Selector, Verdict};
use smoltcp::iface::{Interface, SocketHandle as Seat, SocketSet};
use smoltcp::phy::{Medium, TunTapInterface, wait};
use smoltcp::socket::tcp;
use smoltcp::wire::IpListenEndpoint;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: usize = 65536;
const HALF: Duration = Duration::from_secs(600);
const FLOOR: f64 = 0.05;
const REST: Duration = Duration::from_millis(5);
const DEPTH: usize = 32;
const SWEEP: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct Served {
    pub accepted: usize,
    pub answered: usize,
    pub faulted: usize,
    pub named: usize,
    pub refused: usize,
    pub reaped: usize,
}

impl Served {
    fn gather(&mut self, other: &Self) {
        self.accepted += other.accepted;
        self.answered += other.answered;
        self.faulted += other.faulted;
        self.named += other.named;
        self.refused += other.refused;
        self.reaped += other.reaped;
    }
}

struct Weft<'a> {
    device: &'a mut TunTapInterface,
    iface: &'a mut Interface,
    started: Instant,
    span: Duration,
}

struct Loom<'a> {
    sockets: SocketSet<'a>,
    watching: HashMap<Seat, u16>,
    strands: HashMap<Seat, Strand>,
    burrow: Burrow<'a>,
    pools: &'a Pools,
    swept: Duration,
    served: Served,
}

pub fn serve(warren: &Warren, span: Duration) -> Result<Served, Error> {
    let mut device = TunTapInterface::new(warren.instance.get(), Medium::Ip)
        .map_err(|error| Error::new(format!("cannot open the device: {error}")))?;
    let started = Instant::now();
    let mut iface = raise(&mut device, started);
    let pools = gather(warren, started)?;
    Ok(std::thread::scope(|scope| {
        scope.spawn(|| stand(warren, &pools, span));
        let mut weft = Weft {
            device: &mut device,
            iface: &mut iface,
            started,
            span,
        };
        let mut loom = Loom {
            sockets: SocketSet::new(Vec::new()),
            watching: HashMap::new(),
            strands: HashMap::new(),
            burrow: Burrow::new(&pools),
            pools: &pools,
            swept: Duration::ZERO,
            served: Served::default(),
        };
        loom.run(warren, &mut weft);
        let mut served = loom.served;
        served.gather(&loom.burrow.served);
        served
    }))
}

pub(super) fn gather(warren: &Warren, started: Instant) -> Result<Pools, Error> {
    let policy = Policy::new(HALF, FLOOR)?;
    let mut pools = Pools::new();
    for cluster in warren.clusters {
        let selector = Selector::new(cluster, policy, started);
        pools.insert(cluster.name().clone(), Mutex::new(selector));
    }
    Ok(pools)
}

pub(super) fn stand(warren: &Warren, pools: &Pools, span: Duration) {
    let Err(error) = warden::attend(warren, pools, span) else {
        return;
    };
    (warren.told)(&format!("the resolver refused to start: {error}"));
}

impl Loom<'_> {
    fn run(&mut self, warren: &Warren, weft: &mut Weft) {
        while lasting(weft.started, weft.span) {
            self.replenish(warren.ports);
            self.burrow.hold(&mut self.sockets, warren.ports);
            weft.iface
                .poll(beat(weft.started), weft.device, &mut self.sockets);
            self.weave(warren);
            self.burrow.drain(warren, &mut self.sockets);
            self.burrow.spill(warren, &mut self.sockets);
            self.burrow.reap(warren);
            self.reap(warren);
            self.sweep(warren, weft.started.elapsed());
            let _ = wait(weft.device.as_raw_fd(), Some(REST.into()));
        }
    }

    fn replenish(&mut self, ports: &[u16]) {
        self.watching
            .retain(|seat, _| self.sockets.get::<tcp::Socket>(*seat).state() != tcp::State::Closed);
        for port in ports {
            let held = self.standing(*port);
            for _ in held..DEPTH {
                self.watch(*port);
            }
        }
    }

    fn standing(&self, port: u16) -> usize {
        self.watching
            .iter()
            .filter(|(_, held)| **held == port)
            .filter(|(seat, _)| self.sockets.get::<tcp::Socket>(**seat).is_listening())
            .count()
    }

    fn sweep(&mut self, warren: &Warren, since: Duration) {
        if since < self.swept + SWEEP {
            return;
        }
        self.swept = since;
        let Ok(mut router) = warren.router.lock() else {
            return;
        };
        let gone = router.forget(Instant::now());
        drop(router);
        for address in &gone {
            crate::host::Route::release(warren.instance, &address.to_string());
        }
        if !gone.is_empty() {
            (warren.told)(&format!("released {} expired routes", gone.len()));
        }
    }

    fn reap(&mut self, warren: &Warren) {
        let done: Vec<Seat> = self
            .strands
            .iter()
            .filter(|(seat, strand)| self.finished(**seat, strand))
            .map(|(seat, _)| *seat)
            .collect();
        for seat in done {
            if let Some(strand) = self.strands.get(&seat).filter(|held| !held.settled) {
                (warren.told)(&format!(
                    "gone after {}ms with no verdict",
                    strand.opened.elapsed().as_millis()
                ));
            }
            self.strands.remove(&seat);
            self.sockets.remove(seat);
            self.served.reaped += 1;
        }
    }

    fn finished(&self, seat: Seat, strand: &Strand) -> bool {
        let closed = self.sockets.get::<tcp::Socket>(seat).state() == tcp::State::Closed;
        closed && (strand.settled || strand.spent)
    }

    fn watch(&mut self, port: u16) {
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; WINDOW]),
            tcp::SocketBuffer::new(vec![0; WINDOW]),
        );
        let endpoint = IpListenEndpoint { addr: None, port };
        if socket.listen(endpoint).is_err() {
            return;
        }
        let seat = self.sockets.add(socket);
        self.watching.insert(seat, port);
    }

    fn weave(&mut self, warren: &Warren) {
        for seat in self.watching.keys().copied().collect::<Vec<Seat>>() {
            self.adopt(seat, warren);
        }
        for seat in self.strands.keys().copied().collect::<Vec<Seat>>() {
            self.shuttle(seat, warren);
        }
    }

    fn adopt(&mut self, seat: Seat, warren: &Warren) {
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        if socket.state() != tcp::State::Established {
            return;
        }
        let Some(target) = socket.local_endpoint() else {
            return;
        };
        let Ok(router) = warren.router.lock() else {
            return;
        };
        let decision = router.reached(target.addr.into(), Instant::now());
        drop(router);
        self.watching.remove(&seat);
        if !self.begin(seat, warren, decision.cluster()) {
            self.refuse(seat, warren, &decision);
        }
        if decision.ground() == Ground::Named {
            self.served.named += 1;
        }
    }

    fn refuse(&mut self, seat: Seat, warren: &Warren, decision: &dynet_core::Decision) {
        (warren.told)(&format!(
            "closed: cluster {} could not take it",
            decision.cluster().get()
        ));
        self.served.refused += 1;
        self.sockets.get_mut::<tcp::Socket>(seat).abort();
    }

    fn begin(&mut self, seat: Seat, warren: &Warren, wanted: &Name) -> bool {
        let Some(chosen) = warren.passage(self.pools, wanted, Bearing::Stream) else {
            return false;
        };
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        let Some(target) = socket.local_endpoint() else {
            return false;
        };
        (warren.told)(&format!("{target} through {}", chosen.told));
        self.served.accepted += 1;
        let link = Link::open(chosen.passage, target.addr.to_string(), target.port);
        self.strands.insert(seat, Strand::new(link, chosen.charge));
        true
    }

    fn shuttle(&mut self, seat: Seat, warren: &Warren) {
        let Some(strand) = self.strands.get_mut(&seat) else {
            return;
        };
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        if let Some(verdict) = strand.link.verdict() {
            strand.faulted = matches!(verdict, Verdict::Faulted(_));
            strand.settled = true;
            warren::observed(self.pools, &strand.charge, verdict);
            (warren.told)(&format!(
                "{} verdict {verdict:?} after {}ms",
                strand.charge.label.get(),
                strand.opened.elapsed().as_millis()
            ));
            match strand.faulted {
                true => self.served.faulted += 1,
                false => self.served.answered += 1,
            }
        }
        upward(socket, strand);
        downward(socket, strand);
    }
}
