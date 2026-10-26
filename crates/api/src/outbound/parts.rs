use super::shadow::{Cloak, Shroud, veil};
use super::vless::{self, Reality};
use super::vmess::{self, Endpoint, Tunnel};
use crate::manifest::Entry;
use dynet_core::{Error, Fault};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

const PATIENCE: Duration = Duration::from_secs(5);
const ROOM: usize = 16384;

pub enum Egress {
    Vmess(Box<vmess::Egress>),
    Vless(vless::Egress),
    Raw(TcpStream),
}

pub enum Ingress {
    Vmess(Box<vmess::Ingress>),
    Vless(vless::Ingress),
    Raw(TcpStream),
}

impl Egress {
    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        match self {
            Self::Vmess(egress) => egress.send(body),
            Self::Vless(egress) => egress.send(body),
            Self::Raw(stream) => stream
                .write_all(body)
                .map_err(|error| Error::new(format!("cannot send directly: {error}"))),
        }
    }

    pub fn done(&mut self) -> Result<(), Error> {
        match self {
            Self::Vmess(egress) => egress.done(),
            Self::Vless(egress) => egress.done(),
            Self::Raw(stream) => stream
                .shutdown(Shutdown::Write)
                .map_err(|error| Error::new(format!("cannot close directly: {error}"))),
        }
    }
}

impl Ingress {
    pub fn receive(&mut self) -> Result<Option<Vec<u8>>, Error> {
        match self {
            Self::Vmess(ingress) => ingress.receive(),
            Self::Vless(ingress) => ingress.receive(),
            Self::Raw(stream) => {
                let mut room = vec![0; ROOM];
                let size = stream
                    .read(&mut room)
                    .map_err(|error| Error::new(format!("cannot receive directly: {error}")))?;
                room.truncate(size);
                Ok((size > 0).then_some(room))
            }
        }
    }

    pub fn fault(&self) -> Option<Fault> {
        match self {
            Self::Vmess(ingress) => ingress.fault(),
            Self::Vless(ingress) => ingress.fault(),
            Self::Raw(_) => None,
        }
    }
}

impl Write for Egress {
    fn write(&mut self, body: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Vmess(egress) => egress.write(body),
            Self::Vless(egress) => egress.write(body),
            Self::Raw(stream) => stream.write(body),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Read for Ingress {
    fn read(&mut self, room: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Vmess(ingress) => ingress.read(room),
            Self::Vless(ingress) => ingress.read(room),
            Self::Raw(stream) => stream.read(room),
        }
    }
}

pub enum Speaker {
    Plain(Egress),
    Veiled(Egress, Box<Cloak>),
}

pub enum Listener {
    Plain(Ingress),
    Veiled(Ingress, Box<Shroud>),
}

impl Speaker {
    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        match self {
            Self::Plain(egress) => egress.send(body),
            Self::Veiled(egress, cloak) => cloak.send(egress, body),
        }
    }

    pub fn done(&mut self) -> Result<(), Error> {
        match self {
            Self::Plain(egress) | Self::Veiled(egress, _) => egress.done(),
        }
    }
}

impl Listener {
    pub fn receive(&mut self) -> Result<Option<Vec<u8>>, Error> {
        match self {
            Self::Plain(ingress) => ingress.receive(),
            Self::Veiled(ingress, shroud) => shroud.receive(ingress),
        }
    }

    pub fn fault(&self) -> Option<Fault> {
        match self {
            Self::Plain(ingress) | Self::Veiled(ingress, _) => ingress.fault(),
        }
    }
}

pub struct Exit {
    pub host: String,
    pub port: u16,
    pub secret: [u8; 16],
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
        match self {
            Self::Vmess(endpoint) => {
                let mut tunnel = Tunnel::dial(endpoint).map_err(|_| Fault::Reach)?;
                tunnel
                    .board(endpoint, host, port)
                    .map_err(|_| Fault::Handshake)?;
                let (egress, ingress) = tunnel.split();
                Ok((
                    Egress::Vmess(Box::new(egress)),
                    Ingress::Vmess(Box::new(ingress)),
                ))
            }
            Self::Vless(reality) => {
                let stream = reality.connect().map_err(|_| Fault::Reach)?;
                let (egress, ingress) = reality
                    .open(stream, host, port)
                    .map_err(|_| Fault::Handshake)?;
                Ok((Egress::Vless(egress), Ingress::Vless(ingress)))
            }
        }
    }

    pub fn bearing(&self) -> bool {
        matches!(self, Self::Vmess(_))
    }
}

pub enum Passage {
    Plain(Front),
    Veiled(Front, Exit),
    Direct(u32),
}

impl Passage {
    pub fn open(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        match self {
            Self::Plain(front) => {
                let (egress, ingress) = front.board(host, port)?;
                Ok((Speaker::Plain(egress), Listener::Plain(ingress)))
            }
            Self::Veiled(front, exit) => veiled(front, exit, (host, port)),
            Self::Direct(mark) => direct(host, port, *mark),
        }
    }

    pub fn bearing(&self) -> bool {
        matches!(self, Self::Plain(front) if front.bearing())
    }

    pub fn bear(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        let Self::Plain(Front::Vmess(endpoint)) = self else {
            return Err(Fault::Handshake);
        };
        let mut tunnel = Tunnel::dial(endpoint).map_err(|_| Fault::Reach)?;
        tunnel
            .bear(endpoint, host, port)
            .map_err(|_| Fault::Handshake)?;
        let (egress, ingress) = tunnel.split();
        Ok((
            Speaker::Plain(Egress::Vmess(Box::new(egress))),
            Listener::Plain(Ingress::Vmess(Box::new(ingress))),
        ))
    }
}

fn direct(host: &str, port: u16, mark: u32) -> Result<(Speaker, Listener), Fault> {
    let seat = super::bond::place(host, port).map_err(|_| Fault::Reach)?;
    let stream = super::bond::reach(seat, mark, PATIENCE).map_err(|_| Fault::Reach)?;
    let again = stream.try_clone().map_err(|_| Fault::Reach)?;
    Ok((
        Speaker::Plain(Egress::Raw(stream)),
        Listener::Plain(Ingress::Raw(again)),
    ))
}

fn veiled(front: &Front, exit: &Exit, seat: (&str, u16)) -> Result<(Speaker, Listener), Fault> {
    let (mut egress, ingress) = front.board(&exit.host, exit.port)?;
    let (mut cloak, shroud) = veil(exit.secret).map_err(|_| Fault::Handshake)?;
    cloak
        .greet(&mut egress, seat, &[])
        .map_err(|_| Fault::Handshake)?;
    Ok((
        Speaker::Veiled(egress, Box::new(cloak)),
        Listener::Veiled(ingress, Box::new(shroud)),
    ))
}
