use super::link::{Link, Taken};
use crate::outbound::Roster;
use crate::subscription::Entry;
use dynet_core::{Cluster, Error, Instance, Policy, Router, Selector, Verdict};
use smoltcp::iface::{Config, Interface, SocketHandle as Seat, SocketSet};
use smoltcp::phy::{Medium, TunTapInterface, wait};
use smoltcp::socket::tcp;
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{HardwareAddress, IpCidr, IpListenEndpoint, Ipv4Address};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

const WINDOW: usize = 65536;
const HALF: Duration = Duration::from_secs(600);
const FLOOR: f64 = 0.05;
const REST: Duration = Duration::from_millis(5);

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub entries: &'a [Entry],
    pub cluster: &'a Cluster,
    pub router: &'a Router,
    pub ports: &'a [u16],
}

#[derive(Debug, Default)]
pub struct Served {
    pub accepted: usize,
    pub answered: usize,
    pub faulted: usize,
    pub trails: Vec<String>,
}

struct Strand {
    link: Link,
    pending: Vec<u8>,
    hushed: bool,
}

struct Loom<'a> {
    sockets: SocketSet<'a>,
    watching: HashMap<Seat, u16>,
    strands: HashMap<Seat, Strand>,
    selector: Selector,
    served: Served,
}

pub fn serve(warren: &Warren, span: Duration) -> Result<Served, Error> {
    let mut device = TunTapInterface::new(warren.instance.get(), Medium::Ip)
        .map_err(|error| Error::new(format!("cannot open the device: {error}")))?;
    let started = Instant::now();
    let mut iface = raise(&mut device, started);
    let mut loom = Loom {
        sockets: SocketSet::new(Vec::new()),
        watching: HashMap::new(),
        strands: HashMap::new(),
        selector: Selector::new(warren.cluster, Policy::new(HALF, FLOOR)?, started),
        served: Served::default(),
    };
    for port in warren.ports {
        loom.watch(*port);
    }
    while started.elapsed() < span {
        iface.poll(beat(started), &mut device, &mut loom.sockets);
        loom.weave(warren);
        let _ = wait(device.as_raw_fd(), Some(REST.into()));
    }
    Ok(loom.served)
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
        let Some(label) = self.selector.choose(Instant::now()) else {
            return;
        };
        let Ok(endpoint) = Roster::new(warren.entries).board(&label) else {
            return;
        };
        let ground = warren.router.reached(target.addr.into(), Instant::now());
        self.served.trails.push(format!(
            "{target} through {} on ground {:?} to cluster {}",
            label.get(),
            ground.ground(),
            ground.cluster().get()
        ));
        self.served.accepted += 1;
        let link = Link::open(endpoint, target.addr.to_string(), target.port);
        self.strands.insert(
            seat,
            Strand {
                link,
                pending: Vec::new(),
                hushed: false,
            },
        );
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

fn upward(socket: &mut tcp::Socket, strand: &mut Strand) {
    if socket.can_recv() {
        let taken = socket
            .recv(|data| (data.len(), data.to_vec()))
            .unwrap_or_default();
        if !taken.is_empty() {
            strand.link.offer(&taken);
        }
    }
    if !socket.may_recv() && !strand.hushed {
        strand.link.hush();
        strand.hushed = true;
    }
}

fn downward(socket: &mut tcp::Socket, strand: &mut Strand) {
    if strand.pending.is_empty() {
        match strand.link.take() {
            Taken::Body(body) => strand.pending = body,
            Taken::Empty => return,
            Taken::Spent => {
                socket.close();
                return;
            }
        }
    }
    if !socket.can_send() {
        return;
    }
    let written = socket.send_slice(&strand.pending).unwrap_or_default();
    strand.pending.drain(..written);
}
