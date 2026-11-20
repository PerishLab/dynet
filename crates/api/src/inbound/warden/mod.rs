mod admission;
mod call;
mod lookup;
#[cfg(test)]
mod tests;
use admission::Admission;
use call::Call;
mod frame;
mod reply;
mod tcp;
mod udp;
mod wire;
use super::store::{Key, Recall};
use super::warren::{self, Warren};
use crate::host::clock;
use crate::resolver::{Chain, Packet};
use dynet_core::{Bearing, Domain, Error, Name, Verdict};
use reply::Reply;
use std::net::{TcpListener, UdpSocket};
use std::time::Duration;
use wire::through;

const REST: Duration = Duration::from_millis(200);
const ROOM: usize = 1500;
const TRIES: usize = 3;

struct Post<'a> {
    warren: &'a Warren<'a>,
    reply: Reply<'a>,
}

pub(super) fn seat(address: std::net::SocketAddrV4) -> std::io::Result<(UdpSocket, TcpListener)> {
    let udp = UdpSocket::bind(address)?;
    let tcp = TcpListener::bind(udp.local_addr()?)?;
    udp.set_read_timeout(Some(REST))?;
    udp.set_write_timeout(Some(REST))?;
    tcp.set_nonblocking(true)?;
    Ok((udp, tcp))
}

pub fn attend(
    warren: &Warren,
    span: Duration,
    sockets: &(UdpSocket, TcpListener),
) -> Result<(), Error> {
    let admission = Admission::new();
    std::thread::scope(|scope| {
        scope.spawn(|| udp::attend(warren, span, &sockets.0, &admission));
        tcp::attend(warren, span, &sockets.1, &admission);
    });
    Ok(())
}

impl Post<'_> {
    fn answer(&self, call: Call) -> bool {
        let result = self.settle(&call);
        let answered = result.is_ok();
        let spoken = match result {
            Ok(spoken) => spoken,
            Err(error) => {
                let _ = self.speak(&Packet::new(&call.asked).failed(&call.query), &call);
                format!("refused: {error}")
            }
        };
        (self.warren.told)(&spoken);
        answered
    }

    fn settle(&self, call: &Call) -> Result<String, Error> {
        let asking = Packet::new(&call.asked);
        let query = &call.query;
        let admission = self.issue(call, &Chain::default(), None)?;
        if !admission.accepted {
            return self.refuse(call, &admission.text);
        }
        let Some(cluster) = &call.cluster else {
            let mut reply = self.reply.relay(self.warren, call)?;
            let mut chain = Chain::default();
            chain.extend(&call.asked, &reply, clock())?;
            if reply[2] & 2 != 0 {
                reply = asking.barren(query);
                reply[2] |= 2;
            }
            return self
                .issue(call, &chain, Some(&reply))
                .map(|outcome| outcome.text);
        };
        if call.asked[query.span - 2..query.span] != [0, 1] {
            return self.refuse(call, "unsupported managed DNS class");
        }
        if query.hints() {
            return self
                .issue(call, &Chain::default(), Some(&asking.barren(query)))
                .map(|outcome| outcome.text);
        }
        if matches!(query.kind, 5 | 39 | 255) {
            return self.refuse(call, "explicit alias inspection under a managed rule");
        }
        if query.kind != 1 {
            return self.metadata(call, cluster);
        }
        let key = Key {
            stamp: call.stamp.clone(),
            name: Domain::new(&query.name)?.get().to_owned(),
            cluster: cluster.clone(),
        };
        if let Recall::Known {
            mut chain,
            aged,
            refresh,
        } = self.warren.store.recall(&key)
        {
            for item in &mut chain.addresses {
                item.life = item.life.saturating_sub(aged).max(1);
            }
            let reply = chain.project(&call.asked, query)?;
            let said = self.issue(call, &chain, Some(&reply))?;
            if refresh && let Err(error) = self.lookup(call, &key, false) {
                self.warren.store.missed(&key);
                return Ok(format!("{}; refresh refused: {error}", said.text));
            }
            return Ok(said.text);
        }
        self.lookup(call, &key, true)
    }

    fn speak(&self, body: &[u8], call: &Call) -> Result<(), Error> {
        let held = self.warren.held.read().map_err(|_| poisoned())?;
        if held.paused || held.stamp != call.stamp {
            return Err(Error::new("the DNS policy changed before its answer"));
        }
        call.circuit
            .allow(|| self.reply.send(body, call.peer))?
            .map_err(|error| Error::new(format!("cannot answer the caller: {error}")))
    }

    fn carry(&self, framed: &[u8], wanted: &Name) -> Result<(String, Vec<u8>), Error> {
        let mut last = None;
        for _ in 0..TRIES {
            let Some(chosen) = self.warren.passage(wanted, Bearing::Stream) else {
                break;
            };
            let carried = through(&chosen, &self.warren.current().upstream, framed);
            let verdict = match &carried {
                Ok(_) => Verdict::Answered,
                Err(fault) => Verdict::Faulted(*fault),
            };
            warren::observed(chosen.charge.as_ref(), verdict);
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

fn poisoned() -> Error {
    Error::new("a lock was left poisoned by a dead thread")
}
