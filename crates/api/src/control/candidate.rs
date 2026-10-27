use super::{Payload, refused};
use dynet_core::Error;
use secrets::{Assignment, Identity, Kind, Reader, Receipt, Verified};

pub struct Decoder {
    reader: Reader,
    identity: Identity,
}

pub struct Incoming {
    pub assignment: Assignment,
    pub(super) bytes: Vec<u8>,
    verified: Verified,
}

#[derive(Clone)]
pub struct Candidate {
    pub assignment: Assignment,
    pub payload: Option<Payload>,
}

impl Decoder {
    pub fn new(reader: Reader, identity: Identity) -> Self {
        Self { reader, identity }
    }

    pub fn receive(&self, bytes: Vec<u8>, floor: Option<&Receipt>) -> Result<Incoming, Error> {
        let verified = self.reader.verify(&bytes, floor).map_err(|_| refused())?;
        let assignment = Assignment {
            target: verified.target().clone(),
            receipt: verified.receipt().clone(),
            kind: verified.kind(),
        };
        Ok(Incoming {
            assignment,
            bytes,
            verified,
        })
    }

    pub fn open(&self, incoming: Incoming) -> Result<Candidate, Error> {
        let opened = incoming
            .verified
            .open(&self.identity)
            .map_err(|_| refused())?;
        let payload = match opened.kind {
            Kind::Configuration => Some(Payload::read(opened.content())?),
            Kind::Revocation => None,
        };
        Ok(Candidate {
            assignment: incoming.assignment,
            payload,
        })
    }
}
