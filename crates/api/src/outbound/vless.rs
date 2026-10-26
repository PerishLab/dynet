pub mod certificate;
pub mod filter;
mod half;
pub mod handshake;
pub mod hello;
mod message;
pub mod record;
pub mod schedule;
mod session;
pub mod vision;

use super::bond::{place, reach};
use super::raw::{addressed, parse, unbase};
use crate::subscription::Entry;
use dynet_core::Error;
pub use half::{Egress, Ingress};
use std::net::TcpStream;
use std::time::Duration;

const FLOW: &str = "xtls-rprx-vision";
const NEAR: Duration = Duration::from_secs(2);
const STREAM: u8 = 1;

pub struct Reality {
    pub host: String,
    pub port: u16,
    pub identity: [u8; 16],
    pub name: String,
    pub key: [u8; 32],
    pub short: [u8; 8],
    pub vision: bool,
    pub mark: u32,
}

impl Reality {
    pub fn read(entry: &Entry, host: &str, mark: u32) -> Result<Self, Error> {
        let field = |name: &str| {
            entry
                .field(name)
                .ok_or_else(|| Error::new(format!("a REALITY node carries no {name}")))
        };
        let key = unbase(field("reality-opts.public-key")?)
            .and_then(|drawn| <[u8; 32]>::try_from(drawn).ok())
            .ok_or_else(|| Error::new("a REALITY public key is not thirty two bytes"))?;
        let flow = entry.field("flow").unwrap_or_default();
        if !flow.is_empty() && flow != FLOW {
            return Err(Error::new(format!("Dynet does not speak the {flow} flow")));
        }
        Ok(Self {
            host: host.to_string(),
            port: field("port")?
                .parse()
                .map_err(|_| Error::new("a REALITY node carries no usable port"))?,
            identity: parse(field("uuid")?)?,
            name: field("servername")?.to_string(),
            key,
            short: short(entry.field("reality-opts.short-id").unwrap_or_default())?,
            vision: flow == FLOW,
            mark,
        })
    }

    pub fn connect(&self) -> Result<TcpStream, Error> {
        reach(place(&self.host, self.port)?, self.mark, NEAR)
    }

    pub fn open(
        &self,
        stream: TcpStream,
        host: &str,
        port: u16,
    ) -> Result<(Egress, Ingress), Error> {
        let session = handshake::shake(stream, self)?;
        let mut request = vec![0u8];
        request.extend_from_slice(&self.identity);
        match self.vision {
            true => {
                request.extend_from_slice(&[18, 0x0a, 16]);
                request.extend_from_slice(FLOW.as_bytes());
            }
            false => request.push(0),
        }
        request.push(STREAM);
        request.extend_from_slice(&port.to_be_bytes());
        request.extend_from_slice(&addressed(host));
        half::split(session, &request, self.vision.then_some(self.identity))
    }
}

fn short(told: &str) -> Result<[u8; 8], Error> {
    let mut held = [0u8; 8];
    if told.len() > 16 || !told.len().is_multiple_of(2) {
        return Err(Error::new(
            "a REALITY short id must be at most sixteen hex digits",
        ));
    }
    for (slot, pair) in held.iter_mut().zip(told.as_bytes().chunks(2)) {
        let text = std::str::from_utf8(pair).map_err(|_| Error::new("a short id is not text"))?;
        *slot = u8::from_str_radix(text, 16)
            .map_err(|_| Error::new("a short id is not hexadecimal"))?;
    }
    Ok(held)
}
