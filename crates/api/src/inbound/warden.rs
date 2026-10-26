use super::lasting;
use super::store::{Key, Recall};
use super::warren::{self, Chosen, Warren};
use crate::host::{Route, clock};
use crate::outbound::Listener;
use crate::resolver::{self, Answer, Packet, Query};
use dynet_core::{Bearing, Domain, Error, Fault, Name, Verdict, spared};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(200);
const ROOM: usize = 1500;
const TRIES: usize = 3;
const MARK: u16 = 0x7a7a;

struct Call {
    asked: Vec<u8>,
    peer: SocketAddr,
}

struct Post<'a> {
    warren: &'a Warren<'a>,
    socket: &'a UdpSocket,
}

pub fn attend(warren: &Warren, span: Duration, socket: &UdpSocket) -> Result<(), Error> {
    let _ = socket.set_read_timeout(Some(REST));
    let post = Post { warren, socket };
    let started = Instant::now();
    std::thread::scope(|scope| {
        while lasting(started, span) && !warren.halted() {
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
        if query.hints() {
            return self.limited(&asking, call, &query);
        }
        let domain = Domain::new(&query.name)?;
        let Some(cluster) = self.warren.claimed(&domain) else {
            let plain = asking.relay(self.warren.upstream, self.warren.instance.mark())?;
            self.speak(&plain, call)?;
            return Ok(format!("{} left to the upstream", query.name));
        };
        let key = Key {
            name: query.name.clone(),
            cluster: cluster.clone(),
        };
        if let Recall::Known {
            reply,
            aged,
            refresh,
        } = self.warren.store.recall(&key)
        {
            let found = resolver::read(&reply)?;
            self.assure(&domain, &found, &cluster)?;
            let mark = u16::from_be_bytes([call.asked[0], call.asked[1]]);
            self.speak(&resolver::renew(&reply, mark, aged)?, call)?;
            return self.after(&key, &domain, refresh);
        }
        let framed = asking.frame()?;
        let (label, carried) = self.carry(&framed, &cluster)?;
        let Some(reply) = spare(&carried)? else {
            self.speak(&asking.failed(&query), call)?;
            return Ok(format!(
                "{} through {label} answered reserved addresses alone, which are left out; failed",
                query.name
            ));
        };
        let found = resolver::read(&reply)?;
        let told = self.keep(&domain, &found, &cluster)?;
        self.remember(&key, &reply, &found);
        self.speak(&reply, call)?;
        Ok(format!(
            "{} through {label} to cluster {} gave {told}",
            query.name,
            cluster.get()
        ))
    }

    fn limited(&self, asking: &Packet, call: &Call, query: &Query) -> Result<String, Error> {
        if self.carried(&query.name) {
            self.speak(&asking.barren(query), call)?;
            return Ok(format!(
                "{} asked for type {}, answered none because this carries it",
                query.name, query.kind
            ));
        }
        let plain = asking.relay(self.warren.upstream, self.warren.instance.mark())?;
        self.speak(&plain, call)?;
        Ok(format!(
            "{} asked for type {}, left to the upstream",
            query.name, query.kind
        ))
    }

    fn carried(&self, name: &str) -> bool {
        let Ok(domain) = Domain::new(name) else {
            return true;
        };
        self.warren.claimed(&domain).is_some()
    }

    fn after(&self, key: &Key, domain: &Domain, refresh: bool) -> Result<String, Error> {
        if !refresh {
            return Ok(format!("{} from memory", key.name));
        }
        match self.again(key, domain) {
            Ok(label) => Ok(format!(
                "{} from memory, refreshed through {label}",
                key.name
            )),
            Err(error) => {
                let spent = self.warren.store.missed(key);
                Ok(format!(
                    "{} from memory, refresh {spent} failed: {error}",
                    key.name
                ))
            }
        }
    }

    fn assure(&self, domain: &Domain, found: &[Answer], cluster: &Name) -> Result<(), Error> {
        let ledger = self.warren.ledger.lock().map_err(|_| poisoned())?;
        let live = found.iter().all(|item| {
            ledger
                .lookup(IpAddr::V4(item.address))
                .is_some_and(|issue| !issue.released())
        });
        drop(ledger);
        if live {
            for item in found {
                capture(self.warren.instance, IpAddr::V4(item.address))?;
            }
            return Ok(());
        }
        self.keep(domain, found, cluster)?;
        Ok(())
    }

    fn again(&self, key: &Key, domain: &Domain) -> Result<String, Error> {
        let asking = resolver::ask(&key.name, MARK)?;
        let (label, framed) = self.carry(&asking, &key.cluster)?;
        let reply = spare(&framed)?
            .ok_or_else(|| Error::new("the refreshed answer held reserved addresses alone"))?;
        let found = resolver::read(&reply)?;
        self.keep(domain, &found, &key.cluster)?;
        self.remember(key, &reply, &found);
        Ok(label)
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

    fn keep(&self, domain: &Domain, found: &[Answer], cluster: &Name) -> Result<usize, Error> {
        let life = found.iter().map(|item| u64::from(item.life)).max();
        let told = (life.unwrap_or_default(), clock());
        let mut ledger = self.warren.ledger.lock().map_err(|_| poisoned())?;
        let mut held = 0;
        for item in found {
            let address = IpAddr::V4(item.address);
            if ledger.record(address, (domain, cluster), told) {
                self.warren.journal.stain();
                capture(self.warren.instance, address)?;
                held += 1;
            }
        }
        drop(ledger);
        self.warren.journal.stain();
        Ok(held)
    }

    fn carry(&self, framed: &[u8], wanted: &Name) -> Result<(String, Vec<u8>), Error> {
        let mut last = None;
        for _ in 0..TRIES {
            let Some(chosen) = self.warren.passage(wanted, Bearing::Stream) else {
                break;
            };
            let carried = through(&chosen, self.warren.upstream, framed);
            let verdict = match &carried {
                Ok(_) => Verdict::Answered,
                Err(fault) => Verdict::Faulted(*fault),
            };
            warren::observed(self.warren, chosen.charge.as_ref(), verdict);
            if let Err(fault) = &carried {
                (self.warren.told)(&format!(
                    "{} did not answer ({fault:?}), asking another node",
                    chosen.told
                ));
                last = Some(*fault);
            }
            if let Ok(body) = carried {
                return Ok((chosen.told, body));
            }
        }
        match last {
            Some(fault) => Err(Error::new(format!(
                "no node of cluster {} answered in {TRIES} tries: {fault:?}",
                wanted.get()
            ))),
            None => Err(Error::new(format!(
                "cluster {} offered no node",
                wanted.get()
            ))),
        }
    }
}

fn through(chosen: &Chosen, upstream: &str, framed: &[u8]) -> Result<Vec<u8>, Fault> {
    let (mut speaker, mut listener) = chosen.passage.open(upstream, 53)?;
    speaker.send(framed).map_err(|_| Fault::Severed)?;
    gather(&mut listener).map_err(|_| listener.fault().unwrap_or(Fault::Silent))
}

fn spare(framed: &[u8]) -> Result<Option<Vec<u8>>, Error> {
    let reply = framed
        .get(2..)
        .ok_or_else(|| Error::new("the upstream answer carried no body"))?;
    let Some(pruned) = resolver::prune(reply, |address| spared(IpAddr::V4(address)))? else {
        return Ok(Some(reply.to_vec()));
    };
    match resolver::read(&pruned)?.is_empty() {
        true => Ok(None),
        false => Ok(Some(pruned)),
    }
}

fn poisoned() -> Error {
    Error::new("a lock was left poisoned by a dead thread")
}

fn capture(instance: &dynet_core::Instance, address: IpAddr) -> Result<(), Error> {
    match Route::hold(instance, address) {
        true => Ok(()),
        false => Err(Error::new(format!(
            "cannot capture {address}, so its answer is withheld"
        ))),
    }
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
