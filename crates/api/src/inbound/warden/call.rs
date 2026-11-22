use crate::inbound::Held;
use crate::outbound::Circuit;
use crate::resolver::{Packet, Query};
use dynet_core::{Domain, Error, Name, Rule, Subject};
use std::net::SocketAddr;
use std::sync::Arc;

pub(super) struct Call {
    pub asked: Vec<u8>,
    pub peer: SocketAddr,
    pub stamp: String,
    pub generation: u64,
    pub circuit: Arc<Circuit>,
    pub source: std::cell::Cell<&'static str>,
    pub rule: Option<Subject>,
    pub query: Query,
    pub cluster: Option<Name>,
}

impl Call {
    pub fn new(held: &Held, asked: Vec<u8>, peer: SocketAddr) -> Result<Self, Error> {
        let query = Packet::new(&asked).asked()?;
        let domain = Domain::new(&query.name)?;
        let rule = held.loaded.manifest().table().matching(&domain);
        let cluster = rule.map(Rule::cluster).cloned();
        Ok(Self {
            rule: rule.map(Rule::subject).cloned(),
            asked,
            peer,
            stamp: held.stamp.clone(),
            generation: held.generation,
            circuit: held.fence.ticket()?,
            source: std::cell::Cell::new(if cluster.is_some() {
                "cluster"
            } else {
                "upstream"
            }),
            query,
            cluster,
        })
    }
}
