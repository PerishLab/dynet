use super::blame;
use super::chunk::Chunk;
use super::kdf;
use super::shadow::{Cloak, Shroud, veil};
use super::vmess::{Endpoint, Tunnel};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use dynet_core::{Error, Fault};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const BORNE: Duration = Duration::from_secs(600);

pub struct Egress {
    stream: TcpStream,
    outbound: Chunk,
}

pub struct Ingress {
    stream: TcpStream,
    spare: Vec<u8>,
    inbound: Option<Chunk>,
    reply: [u8; 16],
    seed: [u8; 16],
    verify: u8,
    fault: Option<Fault>,
}

impl Egress {
    pub(super) fn new(stream: TcpStream, outbound: Chunk) -> Self {
        Self { stream, outbound }
    }

    pub(super) fn greet(&mut self, request: &[u8]) -> Result<(), Error> {
        self.stream
            .write_all(request)
            .map_err(|error| Error::new(format!("cannot greet the node: {error}")))
    }

    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        self.outbound.send(&mut self.stream, body)
    }

    pub fn done(&self) -> Result<(), Error> {
        self.stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|error| Error::new(format!("cannot close the node session: {error}")))
    }
}

impl Ingress {
    pub(super) fn new(stream: TcpStream, reply: [u8; 16], seed: [u8; 16], verify: u8) -> Self {
        Self {
            stream,
            spare: Vec::new(),
            inbound: None,
            reply,
            seed,
            verify,
            fault: None,
        }
    }

    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    pub fn receive(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.inbound.is_none() {
            self.accept()?;
        }
        let Some(inbound) = self.inbound.as_mut() else {
            return Ok(None);
        };
        inbound.receive(&mut self.stream)
    }

    fn accept(&mut self) -> Result<(), Error> {
        let outcome = self.settle();
        if outcome.is_err() && self.fault.is_none() {
            self.fault = Some(Fault::Handshake);
        }
        outcome
    }

    fn settle(&mut self) -> Result<(), Error> {
        let key = shorten(&kdf::digest(&self.reply));
        let seed = shorten(&kdf::digest(&self.seed));
        let length = unseal(
            &kdf::shorten(&key, &[b"AEAD Resp Header Len Key"]),
            &kdf::derive(&seed, &[b"AEAD Resp Header Len IV"]),
            &self.read(18)?,
        )?;
        let size = usize::from(u16::from_be_bytes([length[0], length[1]]));
        let header = unseal(
            &kdf::shorten(&key, &[b"AEAD Resp Header Key"]),
            &kdf::derive(&seed, &[b"AEAD Resp Header IV"]),
            &self.read(size + 16)?,
        )?;
        if header.first() != Some(&self.verify) {
            return Err(Error::new(
                "the node answered with the wrong session marker; the handshake was not accepted",
            ));
        }
        self.inbound = Some(Chunk::new(&key, seed));
        let _ = self.stream.set_read_timeout(Some(BORNE));
        Ok(())
    }

    fn read(&mut self, size: usize) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; size];
        let stream = &mut self.stream;
        let outcome = stream.read_exact(&mut buffer);
        if let Err(error) = outcome {
            self.fault = Some(blame::ending(error.kind()));
            return Err(Error::new(format!(
                "the node closed before answering: {error}"
            )));
        }
        Ok(buffer)
    }
}

fn shorten(value: &[u8; 32]) -> [u8; 16] {
    let mut short = [0u8; 16];
    short.copy_from_slice(&value[..16]);
    short
}

fn unseal(key: &[u8; 16], seed: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, Error> {
    let cipher = Aes128Gcm::new(Key::<Aes128Gcm>::from_slice(key));
    cipher
        .decrypt(
            Nonce::from_slice(&seed[..12]),
            Payload {
                msg: sealed,
                aad: b"",
            },
        )
        .map_err(|_| Error::new("the node's answer did not open"))
}

impl Write for Egress {
    fn write(&mut self, body: &[u8]) -> std::io::Result<usize> {
        match self.send(body) {
            Ok(()) => Ok(body.len()),
            Err(error) => Err(std::io::Error::other(error.to_string())),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Read for Ingress {
    fn read(&mut self, room: &mut [u8]) -> std::io::Result<usize> {
        while self.spare.is_empty() {
            let taken = self
                .receive()
                .map_err(|error| std::io::Error::other(error.to_string()));
            match taken? {
                None => return Ok(0),
                Some(part) => self.spare = part,
            }
        }
        let size = room.len().min(self.spare.len());
        room[..size].copy_from_slice(&self.spare[..size]);
        self.spare.drain(..size);
        Ok(size)
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

    pub fn done(&self) -> Result<(), Error> {
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

pub enum Passage {
    Plain(Endpoint),
    Veiled(Endpoint, Exit),
}

impl Passage {
    pub fn open(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        match self {
            Self::Plain(endpoint) => plain(endpoint, host, port),
            Self::Veiled(first, exit) => veiled(first, exit, (host, port)),
        }
    }

    pub fn bearing(&self) -> bool {
        matches!(self, Self::Plain(_))
    }

    pub fn bear(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        let Self::Plain(endpoint) = self else {
            return Err(Fault::Handshake);
        };
        let mut tunnel = Tunnel::dial(endpoint).map_err(|_| Fault::Reach)?;
        tunnel
            .bear(endpoint, host, port)
            .map_err(|_| Fault::Handshake)?;
        let (egress, ingress) = tunnel.split();
        Ok((Speaker::Plain(egress), Listener::Plain(ingress)))
    }
}

fn plain(endpoint: &Endpoint, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
    let mut tunnel = Tunnel::dial(endpoint).map_err(|_| Fault::Reach)?;
    tunnel
        .board(endpoint, host, port)
        .map_err(|_| Fault::Handshake)?;
    let (egress, ingress) = tunnel.split();
    Ok((Speaker::Plain(egress), Listener::Plain(ingress)))
}

fn veiled(first: &Endpoint, exit: &Exit, seat: (&str, u16)) -> Result<(Speaker, Listener), Fault> {
    let mut tunnel = Tunnel::dial(first).map_err(|_| Fault::Reach)?;
    tunnel
        .board(first, &exit.host, exit.port)
        .map_err(|_| Fault::Handshake)?;
    let (mut egress, ingress) = tunnel.split();
    let (mut cloak, shroud) = veil(exit.secret).map_err(|_| Fault::Handshake)?;
    cloak
        .greet(&mut egress, seat, &[])
        .map_err(|_| Fault::Handshake)?;
    Ok((
        Speaker::Veiled(egress, Box::new(cloak)),
        Listener::Veiled(ingress, Box::new(shroud)),
    ))
}
