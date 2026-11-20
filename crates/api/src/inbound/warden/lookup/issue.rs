use super::super::{Call, Post, poisoned, wire::capture};
use crate::control::Scope;
use crate::host::clock;
use crate::resolver::{Chain, Packet};
use dynet_core::{Domain, Error};
use std::net::IpAddr;

pub(in crate::inbound::warden) struct Issuance {
    pub accepted: bool,
    pub text: String,
}

impl Post<'_> {
    pub(in crate::inbound::warden) fn refuse(
        &self,
        call: &Call,
        reason: &str,
    ) -> Result<String, Error> {
        let mut body = Packet::new(&call.asked).barren(&call.query);
        body[3] = 0x85;
        self.speak(&body, call)?;
        Ok(format!("{} refused: {reason}", call.query.name))
    }

    pub(in crate::inbound::warden) fn issue(
        &self,
        call: &Call,
        chain: &Chain,
        reply: Option<&[u8]>,
    ) -> Result<Issuance, Error> {
        let root = Domain::new(&call.query.name)?;
        let mut ledger = self.warren.ledger.lock().map_err(|_| poisoned())?;
        let held = self.warren.held.read().map_err(|_| poisoned())?;
        if held.paused || held.stamp != call.stamp {
            return Err(Error::new("DNS policy changed before issuance"));
        }
        let scope = Scope {
            root: &root,
            cluster: call.cluster.as_ref(),
            generation: &call.stamp,
            table: held.loaded.manifest().table(),
        };
        let now = clock();
        let decision = self.warren.control.cache.origins(&scope, chain, now)?;
        if let Err(reason) = decision {
            if reply.is_some() {
                let mut denied = Packet::new(&call.asked).barren(&call.query);
                denied[3] = 0x85;
                call.circuit
                    .allow(|| self.reply.send(&denied, call.peer))?
                    .map_err(|error| Error::new(format!("cannot send DNS refusal: {error}")))?;
            }
            return Ok(Issuance {
                accepted: false,
                text: format!("{} refused: {reason}", root.get()),
            });
        }
        if let Some(cluster) = &call.cluster {
            for item in &chain.addresses {
                let address = IpAddr::V4(item.address);
                ledger.record(address, (&root, cluster), (u64::from(item.life), now));
                self.warren.journal.stain();
                capture(self.warren.instance, address)?;
            }
            if !chain.addresses.is_empty() {
                self.warren.journal.save(&ledger)?;
            }
        }
        if let Some(body) = reply {
            call.circuit
                .allow(|| self.reply.send(body, call.peer))?
                .map_err(|error| Error::new(format!("cannot answer the caller: {error}")))?;
        }
        Ok(Issuance {
            accepted: true,
            text: match call.cluster {
                Some(_) => format!("{} gave {} addresses", root.get(), chain.addresses.len()),
                None => format!("{} left to the upstream", root.get()),
            },
        })
    }
}
