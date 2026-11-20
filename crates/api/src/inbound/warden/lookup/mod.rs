mod issue;
use super::{Call, Key, Post};
use crate::host::clock;
use crate::resolver::{self, Chain, Packet};
use dynet_core::{Domain, Error, Name, spared};
use std::net::IpAddr;
use std::time::{Duration, Instant};

impl Post<'_> {
    pub(super) fn metadata(&self, call: &Call, cluster: &Name) -> Result<String, Error> {
        let (node, framed) = self.carry(&Packet::new(&call.asked).frame()?, cluster)?;
        (self.warren.told)(&format!(
            "{} asked for type {} through {node}",
            call.query.name, call.query.kind
        ));
        let reply = framed
            .get(2..)
            .ok_or_else(|| Error::new("incomplete DNS reply"))?;
        let mut chain = Chain::default();
        chain.extend(&call.asked, reply, clock())?;
        let projection = Chain::metadata(reply, &call.query)?;
        if chain.links.is_empty()
            && let Some(body) = projection
        {
            return self
                .issue(call, &chain, Some(&body))
                .map(|outcome| outcome.text);
        }
        chain.addresses.clear();
        let audit = self.issue(call, &chain, None)?;
        if !audit.accepted {
            return self.refuse(call, &audit.text);
        }
        self.refuse(
            call,
            "metadata cannot be expressed without address or alias data",
        )
    }

    fn denied(&self, call: &Call, reason: String, send: bool) -> Result<String, Error> {
        if send {
            self.refuse(call, &reason)
        } else {
            Err(Error::new(reason))
        }
    }

    pub(super) fn lookup(&self, call: &Call, key: &Key, send: bool) -> Result<String, Error> {
        let mut chain = Chain::default();
        let mut request = Packet::new(&call.asked).frame()?;
        let start = Instant::now();
        let root = Domain::new(&call.query.name)?;
        let (node, reply) = loop {
            if start.elapsed() >= Duration::from_secs(12) || call.circuit.closed() {
                return Err(Error::new("DNS chain exceeded its lookup window"));
            }
            let (node, framed) = self.carry(&request, &key.cluster)?;
            let body = framed
                .get(2..)
                .ok_or_else(|| Error::new("incomplete DNS reply"))?;
            let next = chain.extend(&request[2..], body, clock())?;
            let code = body[3] & 15;
            if body[2] & 2 != 0 {
                return Err(Error::new("truncated DNS reply over the cluster stream"));
            }
            if code != 0 {
                if !chain.links.is_empty() {
                    return Err(Error::new("DNS alias target lookup failed"));
                }
                let mut reply = Packet::new(&call.asked).barren(&call.query);
                reply[3] |= code;
                break (node, reply);
            }
            if let Some(target) = next {
                (self.warren.told)(&format!(
                    "{} names {} through {node}",
                    call.query.name,
                    target.get()
                ));
                if target == root {
                    return Err(Error::new("cyclic DNS alias"));
                }
                let admitted = self.issue(call, &chain, None)?;
                if !admitted.accepted {
                    return self.denied(call, admitted.text, send);
                }
                request = resolver::ask(
                    target.get(),
                    u16::from_be_bytes([call.asked[0], call.asked[1]]),
                )?;
                continue;
            }
            let present = !chain.addresses.is_empty();
            chain
                .addresses
                .retain(|item| !spared(IpAddr::V4(item.address)));
            if present && chain.addresses.is_empty() {
                return Err(Error::new("only reserved IPv4 destinations"));
            }
            break (node, chain.project(&call.asked, &call.query)?);
        };
        let said = self.issue(call, &chain, send.then_some(reply.as_slice()))?;
        let life = chain
            .addresses
            .iter()
            .map(|item| item.life)
            .min()
            .unwrap_or(0);
        if life > 0 && said.accepted {
            self.warren
                .store
                .keep(key.clone(), chain, Duration::from_secs(u64::from(life)));
        }
        if !send && (!said.accepted || life == 0) {
            return Err(Error::new(format!(
                "{} through {node}, no reusable answer",
                said.text
            )));
        }
        Ok(format!(
            "{} through {node} in {}",
            said.text,
            key.cluster.get()
        ))
    }
}
