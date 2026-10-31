use crate::inbound::Held;
use crate::outbound::Circuit;
use crate::resolver::{Packet, Query};
use dynet_core::{Domain, Error, Name};
use std::net::SocketAddr;
use std::sync::Arc;

pub(super) struct Call {
    pub asked: Vec<u8>,
    pub peer: SocketAddr,
    pub stamp: String,
    pub circuit: Arc<Circuit>,
    pub query: Query,
    pub cluster: Option<Name>,
}

impl Call {
    pub fn new(held: &Held, asked: Vec<u8>, peer: SocketAddr) -> Result<Self, Error> {
        let query = Packet::new(&asked).asked()?;
        let domain = Domain::new(&query.name)?;
        let cluster = held.loaded.manifest().table().claimed(&domain).cloned();
        Ok(Self {
            asked,
            peer,
            stamp: held.stamp.clone(),
            circuit: held.fence.ticket()?,
            query,
            cluster,
        })
    }
}
