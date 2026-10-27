use super::refused;
use crate::manifest::{self, Loaded};
use dynet_core::Error;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::net::Ipv4Addr;

#[derive(Clone)]
pub struct Payload {
    pub loaded: Loaded,
    pub upstream: Ipv4Addr,
    pub retain: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire<'a> {
    version: u8,
    #[serde(borrow)]
    manifest: &'a RawValue,
    upstream: Ipv4Addr,
    retain: u64,
}

impl Payload {
    pub fn read(bytes: &[u8]) -> Result<Self, Error> {
        let wire: Wire<'_> = serde_json::from_slice(bytes).map_err(|_| refused())?;
        if wire.version != 1 || wire.retain == 0 {
            return Err(refused());
        }
        if wire.upstream.is_unspecified()
            || wire.upstream.is_multicast()
            || wire.upstream.is_broadcast()
        {
            return Err(refused());
        }
        let loaded = manifest::load(wire.manifest.get()).map_err(|_| refused())?;
        Ok(Self {
            loaded,
            upstream: wire.upstream,
            retain: wire.retain,
        })
    }
}
