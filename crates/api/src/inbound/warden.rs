use super::pump::{Pools, Warren};
use crate::host::Route;
use crate::outbound::{Roster, Tunnel};
use crate::resolver::{self, Answer, Packet, QUAD};
use dynet_core::{Decision, Domain, Error, Ground, Name};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(200);
const ROOM: usize = 1500;
const CUSHION: u64 = 120;

struct Call {
    asked: Vec<u8>,
    peer: SocketAddr,
}

struct Post<'a> {
    warren: &'a Warren<'a>,
    pools: &'a Pools,
    socket: &'a UdpSocket,
}

pub fn attend(warren: &Warren, pools: &Pools, span: Duration) -> Result<(), Error> {
    let socket = UdpSocket::bind(("0.0.0.0", warren.port))
        .map_err(|error| Error::new(format!("cannot hold the resolver port: {error}")))?;
    let _ = socket.set_read_timeout(Some(REST));
    let post = Post {
        warren,
        pools,
        socket: &socket,
    };
    let started = Instant::now();
    std::thread::scope(|scope| {
        while started.elapsed() < span {
            let mut room = [0u8; ROOM];
            let Ok((size, peer)) = socket.recv_from(&mut room) else {
                continue;
            };
            let call = Call {
                asked: room[..size].to_vec(),
                peer,
            };
            scope.spawn(|| post.answer(call));
        }
    });
    Ok(())
}

impl Post<'_> {
    fn answer(&self, call: Call) {
        let spoken = match self.settle(&call) {
            Ok(spoken) => spoken,
            Err(error) => format!("refused: {error}"),
        };
        (self.warren.told)(&spoken);
    }

    fn settle(&self, call: &Call) -> Result<String, Error> {
        let asking = Packet::new(&call.asked);
        let query = asking.asked()?;
        if query.kind == QUAD {
            self.speak(&asking.barren(&query), call)?;
            return Ok(format!(
                "{} asked for a sixth address, answered none",
                query.name
            ));
        }
        let domain = Domain::new(&query.name)?;
        let decision = self.decide(&domain)?;
        if decision.ground() == Ground::Default {
            let plain = asking.relay(self.warren.upstream)?;
            self.speak(&plain, call)?;
            return Ok(format!("{} left to the upstream", query.name));
        }
        let (label, framed) = self.carry(&call.asked, decision.cluster())?;
        let reply = framed
            .get(2..)
            .ok_or_else(|| Error::new("the upstream answer carried no body"))?;
        let found = resolver::read(&framed)?;
        let told = self.keep(&domain, &found, &decision)?;
        self.speak(reply, call)?;
        Ok(format!(
            "{} through {label} to cluster {} on ground {:?} gave {told}",
            query.name,
            decision.cluster().get(),
            decision.ground()
        ))
    }

    fn speak(&self, body: &[u8], call: &Call) -> Result<(), Error> {
        self.socket
            .send_to(body, call.peer)
            .map(|_| ())
            .map_err(|error| Error::new(format!("cannot answer the caller: {error}")))
    }

    fn decide(&self, domain: &Domain) -> Result<Decision, Error> {
        let router = self.warren.router.lock().map_err(|_| poisoned())?;
        Ok(router.asked(domain))
    }

    fn keep(&self, domain: &Domain, found: &[Answer], decision: &Decision) -> Result<usize, Error> {
        let life = found.iter().map(|item| u64::from(item.life)).max();
        let expiry = Instant::now() + Duration::from_secs(life.unwrap_or_default() + CUSHION);
        let addresses: Vec<IpAddr> = found.iter().map(|item| IpAddr::V4(item.address)).collect();
        for address in &addresses {
            Route::hold(self.warren.instance, &address.to_string());
        }
        let mut router = self.warren.router.lock().map_err(|_| poisoned())?;
        router.issued(domain, &addresses, decision, expiry);
        Ok(addresses.len())
    }

    fn carry(&self, asked: &[u8], wanted: &Name) -> Result<(String, Vec<u8>), Error> {
        let pool = self
            .pools
            .get(wanted)
            .ok_or_else(|| Error::new(format!("cluster {} carries no pool here", wanted.get())))?;
        let label = pool
            .lock()
            .map_err(|_| poisoned())?
            .choose(Instant::now())
            .ok_or_else(|| Error::new("the cluster offered no node"))?;
        let endpoint = Roster::new(self.warren.entries).posted(
            &label,
            self.warren.book,
            self.warren.upstream,
        )?;
        let mut tunnel = Tunnel::open(&endpoint, self.warren.upstream, 53)?;
        tunnel.send(&Packet::new(asked).frame()?)?;
        Ok((label.get().to_string(), gather(&mut tunnel)?))
    }
}

fn poisoned() -> Error {
    Error::new("a lock was left poisoned by a dead thread")
}

fn gather(tunnel: &mut Tunnel) -> Result<Vec<u8>, Error> {
    let mut framed = Vec::new();
    while let Some(part) = tunnel.receive()? {
        framed.extend_from_slice(&part);
        if framed.len() >= 2 {
            let want = usize::from(u16::from_be_bytes([framed[0], framed[1]]));
            if framed.len() >= want + 2 {
                break;
            }
        }
    }
    if framed.len() < 2 {
        return Err(Error::new("the upstream answered nothing"));
    }
    Ok(framed)
}
