use super::link::Link;
use super::strand::{Strand, downward, upward};
use super::warden;
use crate::outbound::{Book, Roster};
use crate::subscription::Entry;
use dynet_core::{Cluster, Error, Ground, Instance, Name, Policy, Router, Selector, Verdict};
use smoltcp::iface::{Config, Interface, SocketHandle as Seat, SocketSet};
use smoltcp::phy::{Medium, TunTapInterface, wait};
use smoltcp::socket::tcp;
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{HardwareAddress, IpCidr, IpListenEndpoint, Ipv4Address};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: usize = 65536;
const HALF: Duration = Duration::from_secs(600);
const FLOOR: f64 = 0.05;
const REST: Duration = Duration::from_millis(5);
const DEPTH: usize = 8;

pub type Pools = HashMap<Name, Mutex<Selector>>;

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub entries: &'a [Entry],
    pub clusters: &'a [Cluster],
    pub router: &'a Mutex<Router>,
    pub ports: &'a [u16],
    pub port: u16,
    pub upstream: &'a str,
    pub book: &'a Book,
    pub told: &'a (dyn Fn(&str) + Sync),
}

#[derive(Debug, Default)]
pub struct Served {
    pub accepted: usize,
    pub answered: usize,
    pub faulted: usize,
    pub named: usize,
    pub refused: usize,
    pub reaped: usize,
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
    pools: &'a Pools,
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
            pools: &pools,
            served: Served::default(),
        };
        loom.run(warren, &mut weft);
        loom.served
    }))
}

fn gather(warren: &Warren, started: Instant) -> Result<Pools, Error> {
    let policy = Policy::new(HALF, FLOOR)?;
    let mut pools = Pools::new();
    for cluster in warren.clusters {
        let selector = Selector::new(cluster, policy, started);
        pools.insert(cluster.name().clone(), Mutex::new(selector));
    }
    Ok(pools)
}

fn stand(warren: &Warren, pools: &Pools, span: Duration) {
    let Err(error) = warden::attend(warren, pools, span) else {
        return;
    };
    (warren.told)(&format!("the resolver refused to start: {error}"));
}

fn beat(started: Instant) -> Beat {
    Beat::from_micros(i64::try_from(started.elapsed().as_micros()).unwrap_or_default())
}

fn raise(device: &mut TunTapInterface, started: Instant) -> Interface {
    let config = Config::new(HardwareAddress::Ip);
    let mut iface = Interface::new(config, device, beat(started));
    iface.set_any_ip(true);
    iface.update_ip_addrs(|addresses| {
        let own = IpCidr::new(Ipv4Address::new(198, 51, 100, 1).into(), 24);
        let _ = addresses.push(own);
    });
    let _ = iface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(198, 51, 100, 2));
    iface
}

impl Loom<'_> {
    fn run(&mut self, warren: &Warren, weft: &mut Weft) {
        while weft.started.elapsed() < weft.span {
            self.replenish(warren.ports);
            weft.iface
                .poll(beat(weft.started), weft.device, &mut self.sockets);
            self.weave(warren);
            self.reap();
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

    fn reap(&mut self) {
        let done: Vec<Seat> = self
            .strands
            .iter()
            .filter(|(seat, strand)| self.finished(**seat, strand))
            .map(|(seat, _)| *seat)
            .collect();
        for seat in done {
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
        match self.pools.get(decision.cluster()) {
            None => self.refuse(seat, warren, &decision),
            Some(pool) => self.begin(seat, warren, pool),
        }
        if decision.ground() == Ground::Named {
            self.served.named += 1;
        }
    }

    fn refuse(&mut self, seat: Seat, warren: &Warren, decision: &dynet_core::Decision) {
        (warren.told)(&format!(
            "closed: cluster {} carries no pool here",
            decision.cluster().get()
        ));
        self.served.refused += 1;
        self.sockets.get_mut::<tcp::Socket>(seat).abort();
    }

    fn begin(&mut self, seat: Seat, warren: &Warren, pool: &Mutex<Selector>) {
        let Some(label) = pool
            .lock()
            .ok()
            .and_then(|mut held| held.choose(Instant::now()))
        else {
            return;
        };
        let posted = Roster::new(warren.entries).posted(&label, warren.book, warren.upstream);
        let Ok(endpoint) = posted else {
            return;
        };
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        let Some(target) = socket.local_endpoint() else {
            return;
        };
        (warren.told)(&format!("{target} through {}", label.get()));
        self.served.accepted += 1;
        let link = Link::open(endpoint, target.addr.to_string(), target.port);
        self.strands.insert(seat, Strand::new(link));
    }

    fn shuttle(&mut self, seat: Seat, warren: &Warren) {
        let Some(strand) = self.strands.get_mut(&seat) else {
            return;
        };
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        if let Some(verdict) = strand.link.verdict() {
            strand.faulted = matches!(verdict, Verdict::Faulted(_));
            strand.settled = true;
            (warren.told)(&format!("verdict {verdict:?}"));
            match strand.faulted {
                true => self.served.faulted += 1,
                false => self.served.answered += 1,
            }
        }
        upward(socket, strand);
        downward(socket, strand);
    }
}
