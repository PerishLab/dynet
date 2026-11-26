use super::{Circuit, Egress, Failure, Ingress, Stage};
use crate::manifest::Entry;
use crate::outbound::{
    vless::Reality,
    vmess::{Endpoint, Tunnel},
};
use dynet_core::{Error, Fault};

pub struct Exit {
    pub host: String,
    pub port: u16,
    pub secret: [u8; 16],
    pub relay: bool,
}

pub enum Front {
    Vmess(Endpoint),
    Vless(Reality),
}

impl Front {
    pub fn read(entry: &Entry, placed: String, mark: u32) -> Result<Self, Error> {
        if entry.field("type") == Some("vless") {
            return Ok(Self::Vless(Reality::read(entry, &placed, mark)?));
        }
        let port = entry
            .field("port")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::new("a node carries no usable port"))?;
        let uuid = entry
            .field("uuid")
            .ok_or_else(|| Error::new("no identity"))?;
        Ok(Self::Vmess(Endpoint::new(placed, port, uuid, mark)?))
    }

    pub fn dial(&self, host: &str, port: u16) -> Result<(Egress, Ingress), Error> {
        match self {
            Self::Vmess(endpoint) => {
                let mut tunnel = Tunnel::dial(endpoint)?;
                tunnel.board(endpoint, host, port)?;
                let (egress, ingress) = tunnel.split();
                Ok((
                    Egress::Vmess(Box::new(egress)),
                    Ingress::Vmess(Box::new(ingress)),
                ))
            }
            Self::Vless(reality) => {
                let (egress, ingress) = reality.open(reality.connect()?, host, port)?;
                Ok((Egress::Vless(egress), Ingress::Vless(ingress)))
            }
        }
    }

    pub fn board(&self, host: &str, port: u16) -> Result<(Egress, Ingress), Fault> {
        self.boarding(host, port, None)
            .map_err(|failure| failure.fault)
    }

    pub(super) fn boarding(
        &self,
        host: &str,
        port: u16,
        gate: Option<&Circuit>,
    ) -> Result<(Egress, Ingress), Failure> {
        let connect = |error| Stage::Connect.failed(Fault::Reach, error);
        let handshake = |error| Stage::Handshake.failed(Fault::Handshake, error);
        match self {
            Self::Vmess(endpoint) => {
                let mut tunnel = Tunnel::connect(endpoint, gate).map_err(connect)?;
                tunnel.board(endpoint, host, port).map_err(handshake)?;
                let (egress, ingress) = tunnel.split();
                Ok((
                    Egress::Vmess(Box::new(egress)),
                    Ingress::Vmess(Box::new(ingress)),
                ))
            }
            Self::Vless(reality) => {
                let stream = reality.socket(gate).map_err(connect)?;
                let session = reality.handshake(stream).map_err(handshake)?;
                let (egress, ingress) = reality
                    .request(session, host, port)
                    .map_err(|error| Stage::Open.failed(Fault::Handshake, error))?;
                Ok((Egress::Vless(egress), Ingress::Vless(ingress)))
            }
        }
    }

    pub fn bearing(&self) -> bool {
        matches!(self, Self::Vmess(_))
    }
}
