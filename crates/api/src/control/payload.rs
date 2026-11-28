use super::refused;
use crate::manifest::export::Spec;
use crate::manifest::export::spec::Wire as Projection;
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
    pub projection: Option<Spec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire<'a> {
    version: u8,
    #[serde(borrow)]
    manifest: &'a RawValue,
    upstream: Ipv4Addr,
    retain: u64,
    #[serde(default)]
    projection: Option<Projection>,
}

const LIMIT: usize = 1024 * 1024;

impl Payload {
    pub fn read(bytes: &[u8]) -> Result<Self, Error> {
        Self::parse(bytes).map_err(|_| refused())
    }

    pub fn check(bytes: &[u8]) -> Result<Self, Error> {
        Self::parse(bytes)
    }

    fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > LIMIT {
            return Err(Error::new(format!(
                "business configuration exceeds the {LIMIT}-byte payload bound"
            )));
        }
        let wire: Wire<'_> = serde_json::from_slice(bytes).map_err(|error| {
            Error::new(format!(
                "the business configuration does not parse: {error}"
            ))
        })?;
        if wire.version != 1 {
            return Err(Error::new(format!(
                "business configuration version {} is not version 1",
                wire.version
            )));
        }
        if wire.retain == 0 {
            return Err(Error::new(
                "business configuration retain must be a positive number of seconds",
            ));
        }
        if wire.upstream.is_unspecified()
            || wire.upstream.is_multicast()
            || wire.upstream.is_broadcast()
        {
            return Err(Error::new(format!(
                "business configuration upstream {} must be a unicast IPv4 address",
                wire.upstream
            )));
        }
        let loaded = manifest::load(wire.manifest.get())?;
        let projection = wire.projection.map(Spec::parse).transpose()?;
        Ok(Self {
            loaded,
            upstream: wire.upstream,
            retain: wire.retain,
            projection,
        })
    }
}
