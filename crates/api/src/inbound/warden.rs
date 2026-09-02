use super::clock::lasting;
use super::store::{Key, Recall};
use super::warren::{self, Chosen, Pools, Warren};
use crate::host::Route;
use crate::outbound::Listener;
use crate::resolver::{self, Answer, Packet, QUAD};
use dynet_core::{Bearing, Decision, Domain, Error, Fault, Ground, Name, Verdict};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(200);
const ROOM: usize = 1500;
const CUSHION: u64 = 120;
const MARK: u16 = 0x7a7a;

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
        while lasting(started, span) {
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
        let key = Key {
            name: query.name.clone(),
            cluster: decision.cluster().clone(),
        };
        let remembered = match decision.ground() {
            Ground::Default => Recall::Absent,
            _ => self.warren.store.recall(&key),
        };
        if let Recall::Known {
            reply,
            aged,
            refresh,
        } = remembered
        {
            let mark = u16::from_be_bytes([call.asked[0], call.asked[1]]);
            self.speak(&resolver::renew(&reply, mark, aged)?, call)?;
            return self.after(&key, &domain, &decision, refresh);
        }
        if decision.ground() == Ground::Default {
            let plain = asking.relay(self.warren.upstream)?;
            self.speak(&plain, call)?;
            return Ok(format!("{} left to the upstream", query.name));
        }
        let asking = Packet::new(&call.asked).frame()?;
        let (label, framed) = self.carry(&asking, decision.cluster())?;
        let reply = framed
            .get(2..)
            .ok_or_else(|| Error::new("the upstream answer carried no body"))?;
        let found = resolver::read(reply)?;
        let told = self.keep(&domain, &found, &decision)?;
        self.remember(&key, reply, &found);
        self.speak(reply, call)?;
        Ok(format!(
            "{} through {label} to cluster {} on ground {:?} gave {told}",
            query.name,
            decision.cluster().get(),
            decision.ground()
        ))
    }

    fn after(
        &self,
        key: &Key,
        domain: &Domain,
        decision: &Decision,
        refresh: bool,
    ) -> Result<String, Error> {
        if !refresh {
            return Ok(format!("{} from memory", key.name));
        }
        match self.again(key, domain, decision) {
            Ok(()) => Ok(format!("{} from memory, refreshed", key.name)),
            Err(error) => {
                let spent = self.warren.store.missed(key);
                Ok(format!(
                    "{} from memory, refresh {spent} failed: {error}",
                    key.name
                ))
            }
        }
    }

    fn again(&self, key: &Key, domain: &Domain, decision: &Decision) -> Result<(), Error> {
        let asking = resolver::ask(&key.name, MARK)?;
        let (_, framed) = self.carry(&asking, decision.cluster())?;
        let reply = framed
            .get(2..)
            .ok_or_else(|| Error::new("the refreshed answer carried no body"))?;
        let found = resolver::read(reply)?;
        self.keep(domain, &found, decision)?;
        self.remember(key, reply, &found);
        Ok(())
    }

    fn remember(&self, key: &Key, reply: &[u8], found: &[Answer]) {
        let life = found.iter().map(|item| u64::from(item.life)).min();
        let Some(life) = life else {
            return;
        };
        self.warren
            .store
            .keep(key.clone(), reply.to_vec(), Duration::from_secs(life));
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

    fn carry(&self, framed: &[u8], wanted: &Name) -> Result<(String, Vec<u8>), Error> {
        let chosen = self
            .warren
            .passage(self.pools, wanted, Bearing::Stream)
            .ok_or_else(|| Error::new(format!("cluster {} offered no node", wanted.get())))?;
        let carried = through(&chosen, self.warren.upstream, framed);
        let verdict = match &carried {
            Ok(_) => Verdict::Answered,
            Err(fault) => Verdict::Faulted(*fault),
        };
        warren::observed(self.pools, &chosen.charge, verdict);
        match carried {
            Ok(body) => Ok((chosen.told, body)),
            Err(fault) => Err(Error::new(format!(
                "the resolver could not reach a node: {fault:?}"
            ))),
        }
    }
}

fn through(chosen: &Chosen, upstream: &str, framed: &[u8]) -> Result<Vec<u8>, Fault> {
    let (mut speaker, mut listener) = chosen.passage.open(upstream, 53)?;
    speaker.send(framed).map_err(|_| Fault::Severed)?;
    gather(&mut listener).map_err(|_| listener.fault().unwrap_or(Fault::Silent))
}

fn poisoned() -> Error {
    Error::new("a lock was left poisoned by a dead thread")
}

fn gather(listener: &mut Listener) -> Result<Vec<u8>, Error> {
    let mut framed = Vec::new();
    while let Some(part) = listener.receive()? {
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
