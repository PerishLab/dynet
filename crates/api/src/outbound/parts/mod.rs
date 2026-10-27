use super::shadow::{Cloak, Shroud};
use super::vless;
use super::vmess;
use dynet_core::{Error, Fault};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

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

mod fence;
mod front;
mod passage;
pub use fence::{Circuit, Fence};
pub use front::{Exit, Front};
pub use passage::Passage;
