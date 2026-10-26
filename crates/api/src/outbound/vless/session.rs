use super::record::{ALERT, APPLICATION, CHANGE, Cipher, HANDSHAKE};
use crate::outbound::bond::ending;
use dynet_core::{Error, Fault};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

const ROOM: usize = 16384;
const LIMIT: usize = ROOM + 256;
const UPDATE: u8 = 24;

pub struct Wire {
    stream: TcpStream,
    raw: Vec<u8>,
    pub fault: Option<Fault>,
}

pub struct Record {
    pub kind: u8,
    pub header: Vec<u8>,
    pub body: Vec<u8>,
}

impl Wire {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            raw: Vec::new(),
            fault: None,
        }
    }

    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    fn fill(&mut self) -> Result<bool, Error> {
        let mut room = vec![0u8; ROOM];
        match self.stream.read(&mut room) {
            Ok(0) => Ok(false),
            Ok(size) => {
                self.raw.extend_from_slice(&room[..size]);
                Ok(true)
            }
            Err(error) => {
                self.fault = Some(ending(error.kind()));
                Err(Error::new(format!("the node session broke: {error}")))
            }
        }
    }

    pub fn next(&mut self) -> Result<Option<Record>, Error> {
        loop {
            if self.raw.len() >= 5 {
                let size = usize::from(u16::from_be_bytes([self.raw[3], self.raw[4]]));
                if size > LIMIT {
                    return Err(Error::new("the node sent a record past the limit"));
                }
                if self.raw.len() >= 5 + size {
                    let header: Vec<u8> = self.raw.drain(..5).collect();
                    let body = self.raw.drain(..size).collect();
                    let kind = header[0];
                    return Ok(Some(Record { kind, header, body }));
                }
            }
            if !self.fill()? {
                return match self.raw.is_empty() {
                    true => Ok(None),
                    false => Err(Error::new("the node closed inside a record")),
                };
            }
        }
    }

    pub fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.raw)
    }

    pub fn plain(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if !self.raw.is_empty() {
            return Ok(Some(self.rest()));
        }
        match self.fill()? {
            true => Ok(Some(self.rest())),
            false => Ok(None),
        }
    }
}

pub struct Reader {
    pub wire: Wire,
    cipher: Cipher,
}

pub struct Writer {
    stream: TcpStream,
    cipher: Cipher,
}

impl Reader {
    pub fn new(wire: Wire, cipher: Cipher) -> Self {
        Self { wire, cipher }
    }

    pub fn record(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            let Some(record) = self.wire.next()? else {
                return Ok(None);
            };
            match record.kind {
                CHANGE => continue,
                APPLICATION => {}
                _ => {
                    return Err(Error::new(
                        "the node sent a plaintext record after the handshake",
                    ));
                }
            }
            let (kind, body) = self.cipher.open(&record.header, &record.body)?;
            match kind {
                APPLICATION if body.is_empty() => continue,
                APPLICATION => return Ok(Some(body)),
                HANDSHAKE => self.listen(&body)?,
                ALERT if body.get(1) == Some(&0) => return Ok(None),
                _ => return Err(Error::new(format!("the node raised alert {body:?}"))),
            }
        }
    }

    fn listen(&mut self, body: &[u8]) -> Result<(), Error> {
        if body.first() == Some(&UPDATE) {
            self.cipher.update()?;
        }
        Ok(())
    }
}

impl Writer {
    pub fn new(stream: TcpStream, cipher: Cipher) -> Self {
        Self { stream, cipher }
    }

    pub fn seal(&mut self, body: &[u8]) -> Result<(), Error> {
        let mut out = Vec::new();
        for part in body.chunks(ROOM) {
            out.extend_from_slice(&self.cipher.seal(APPLICATION, part)?);
        }
        self.raw(&out)
    }

    pub fn raw(&mut self, body: &[u8]) -> Result<(), Error> {
        self.stream
            .write_all(body)
            .map_err(|error| Error::new(format!("cannot write to the node: {error}")))
    }

    pub fn close(&mut self) -> Result<(), Error> {
        let alert = self.cipher.seal(ALERT, &[1, 0])?;
        self.raw(&alert)
    }

    pub fn shut(&self) -> Result<(), Error> {
        self.stream
            .shutdown(Shutdown::Write)
            .map_err(|error| Error::new(format!("cannot close the node session: {error}")))
    }
}

pub struct Session {
    pub reader: Reader,
    pub writer: Writer,
}
