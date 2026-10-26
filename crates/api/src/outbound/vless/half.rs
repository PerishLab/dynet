use super::filter::Traffic;
use super::session::{Reader, Session, Writer};
use super::vision;
use dynet_core::{Error, Fault};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const BORNE: Duration = Duration::from_secs(600);

type Shared = Option<Arc<Mutex<Traffic>>>;

pub struct Egress {
    writer: Writer,
    traffic: Shared,
    direct: bool,
}

pub struct Ingress {
    reader: Reader,
    traffic: Shared,
    answered: bool,
    direct: bool,
    spare: Vec<u8>,
    fault: Option<Fault>,
}

fn held(traffic: &Arc<Mutex<Traffic>>) -> MutexGuard<'_, Traffic> {
    traffic
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn split(
    session: Session,
    request: &[u8],
    identity: Option<[u8; 16]>,
) -> Result<(Egress, Ingress), Error> {
    let traffic: Shared = identity.map(|identity| Arc::new(Mutex::new(Traffic::new(identity))));
    let mut first = request.to_vec();
    if let Some(shared) = &traffic {
        for block in vision::shape(&mut held(shared), &[])?.blocks {
            first.extend_from_slice(&block);
        }
    }
    let mut writer = session.writer;
    writer.seal(&first)?;
    let reader = session.reader;
    reader
        .wire
        .stream()
        .set_read_timeout(Some(BORNE))
        .map_err(|error| Error::new(format!("cannot bound the node session: {error}")))?;
    Ok((
        Egress {
            writer,
            traffic: traffic.clone(),
            direct: false,
        },
        Ingress {
            reader,
            traffic,
            answered: false,
            direct: false,
            spare: Vec::new(),
            fault: None,
        },
    ))
}

impl Egress {
    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        if self.direct {
            return self.writer.raw(body);
        }
        let Some(shared) = &self.traffic else {
            return self.writer.seal(body);
        };
        let shaped = vision::shape(&mut held(shared), body)?;
        for block in &shaped.blocks {
            self.writer.seal(block)?;
        }
        self.direct |= shaped.switch;
        Ok(())
    }

    pub fn done(&mut self) -> Result<(), Error> {
        if !self.direct {
            self.writer.close()?;
        }
        self.writer.shut()
    }
}

impl Ingress {
    pub fn fault(&self) -> Option<Fault> {
        self.fault.or(self.reader.wire.fault)
    }

    pub fn receive(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            if self.direct {
                return self.reader.wire.plain();
            }
            let Some(body) = self.reader.record()? else {
                return Ok(None);
            };
            let body = self.answer(body)?;
            let body = self.unshape(body);
            if !body.is_empty() {
                return Ok(Some(body));
            }
        }
    }

    fn answer(&mut self, body: Vec<u8>) -> Result<Vec<u8>, Error> {
        if self.answered {
            return Ok(body);
        }
        let refused =
            |reason: &str| Error::new(format!("the node did not accept the request: {reason}"));
        if body.first() != Some(&0) {
            self.fault = Some(Fault::Handshake);
            return Err(refused("the answer carries the wrong version"));
        }
        let skip = usize::from(*body.get(1).ok_or_else(|| refused("the answer is short"))?);
        self.answered = true;
        Ok(body.get(2 + skip..).unwrap_or_default().to_vec())
    }

    fn unshape(&mut self, body: Vec<u8>) -> Vec<u8> {
        let Some(shared) = &self.traffic else {
            return body;
        };
        if body.is_empty() {
            return body;
        }
        let mut traffic = held(shared);
        let mut body = body;
        if traffic.within || traffic.count > 0 {
            body = vision::unshape(&mut traffic, &body);
            vision::settle(&mut traffic);
        }
        if traffic.count > 0 {
            traffic.filter(&[&body]);
        }
        if traffic.direct {
            body.extend_from_slice(&self.reader.wire.rest());
            self.direct = true;
        }
        body
    }
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
