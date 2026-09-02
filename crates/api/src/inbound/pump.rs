use super::link::Link;
use super::strand::{Strand, downward, upward};
use super::warden;
use crate::outbound::{Book, Roster};
use crate::subscription::Entry;
use dynet_core::{Cluster, Error, Ground, Instance, Policy, Router, Selector, Verdict};
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

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub entries: &'a [Entry],
    pub cluster: &'a Cluster,
    pub router: &'a Mutex<Router>,
    pub ports: &'a [u16],
    pub port: u16,
    pub upstream: &'a str,
    pub book: &'a Book,
}

#[derive(Debug, Default)]
pub struct Served {
    pub accepted: usize,
    pub answered: usize,
    pub faulted: usize,
    pub named: usize,
    pub trails: Vec<String>,
    pub spoken: Vec<String>,
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
    selector: &'a Mutex<Selector>,
    served: Served,
}

pub fn serve(warren: &Warren, span: Duration) -> Result<Served, Error> {
    let mut device = TunTapInterface::new(warren.instance.get(), Medium::Ip)
        .map_err(|error| Error::new(format!("cannot open the device: {error}")))?;
    let started = Instant::now();
    let mut iface = raise(&mut device, started);
    let selector = Mutex::new(Selector::new(
        warren.cluster,
        Policy::new(HALF, FLOOR)?,
        started,
    ));
    let voice = Mutex::new(Vec::new());
    let mut served = std::thread::scope(|scope| {
        scope.spawn(|| stand(warren, &selector, &voice, span));
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
            selector: &selector,
            served: Served::default(),
        };
        for port in warren.ports {
            loom.watch(*port);
        }
        loom.run(warren, &mut weft);
        loom.served
    });
    served.spoken = voice.into_inner().unwrap_or_default();
    Ok(served)
}

fn stand(warren: &Warren, selector: &Mutex<Selector>, voice: &Mutex<Vec<String>>, span: Duration) {
    let Err(error) = warden::attend(warren, selector, voice, span) else {
        return;
    };
    if let Ok(mut held) = voice.lock() {
        held.push(format!("the resolver refused to start: {error}"));
    }
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
            weft.iface
                .poll(beat(weft.started), weft.device, &mut self.sockets);
            self.weave(warren);
            let _ = wait(weft.device.as_raw_fd(), Some(REST.into()));
        }
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
            self.shuttle(seat);
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
        let Some(label) = self
            .selector
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
        let Ok(router) = warren.router.lock() else {
            return;
        };
        let ground = router.reached(target.addr.into(), Instant::now());
        drop(router);
        self.served.trails.push(format!(
            "{target} through {} on ground {:?} to cluster {}",
            label.get(),
            ground.ground(),
            ground.cluster().get()
        ));
        self.served.accepted += 1;
        if ground.ground() == Ground::Named {
            self.served.named += 1;
        }
        let link = Link::open(endpoint, target.addr.to_string(), target.port);
        self.strands.insert(seat, Strand::new(link));
        let port = self.watching.remove(&seat).unwrap_or(target.port);
        self.watch(port);
    }

    fn shuttle(&mut self, seat: Seat) {
        let Some(strand) = self.strands.get_mut(&seat) else {
            return;
        };
        let socket = self.sockets.get_mut::<tcp::Socket>(seat);
        upward(socket, strand);
        downward(socket, strand);
        let Some(verdict) = strand.link.verdict() else {
            return;
        };
        self.served.trails.push(format!("  verdict {verdict:?}"));
        match verdict {
            Verdict::Faulted(_) => self.served.faulted += 1,
            _ => self.served.answered += 1,
        }
    }
}
