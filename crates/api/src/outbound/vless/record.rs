use super::schedule::Hash;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Aes256Gcm};
use chacha20poly1305::ChaCha20Poly1305;
use dynet_core::Error;

pub const HANDSHAKE: u8 = 22;
pub const APPLICATION: u8 = 23;
pub const ALERT: u8 = 21;
pub const CHANGE: u8 = 20;
const NARROW: u16 = 0x1301;
const WIDE: u16 = 0x1302;
const POLY: u16 = 0x1303;
const TAG: usize = 16;

enum Suite {
    Gcm(Box<Aes128Gcm>),
    Wide(Box<Aes256Gcm>),
    Poly(Box<ChaCha20Poly1305>),
}

pub struct Cipher {
    suite: Suite,
    code: u16,
    hash: Hash,
    secret: Vec<u8>,
    seed: Vec<u8>,
    count: u64,
}

impl Suite {
    fn new(code: u16, key: &[u8]) -> Result<Self, Error> {
        let refused = |_| Error::new("a traffic key has the wrong length");
        match code {
            NARROW => Ok(Self::Gcm(Box::new(
                Aes128Gcm::new_from_slice(key).map_err(refused)?,
            ))),
            WIDE => Ok(Self::Wide(Box::new(
                Aes256Gcm::new_from_slice(key).map_err(refused)?,
            ))),
            POLY => Ok(Self::Poly(Box::new(
                ChaCha20Poly1305::new_from_slice(key).map_err(refused)?,
            ))),
            _ => Err(Error::new(format!("the node chose suite {code:#06x}"))),
        }
    }

    fn width(code: u16) -> usize {
        match code {
            NARROW => 16,
            _ => 32,
        }
    }

    fn seal(&self, nonce: &[u8], payload: Payload) -> Option<Vec<u8>> {
        let nonce = nonce.into();
        match self {
            Self::Gcm(cipher) => cipher.encrypt(nonce, payload).ok(),
            Self::Wide(cipher) => cipher.encrypt(nonce, payload).ok(),
            Self::Poly(cipher) => cipher.encrypt(nonce, payload).ok(),
        }
    }

    fn open(&self, nonce: &[u8], payload: Payload) -> Option<Vec<u8>> {
        let nonce = nonce.into();
        match self {
            Self::Gcm(cipher) => cipher.decrypt(nonce, payload).ok(),
            Self::Wide(cipher) => cipher.decrypt(nonce, payload).ok(),
            Self::Poly(cipher) => cipher.decrypt(nonce, payload).ok(),
        }
    }
}

impl Cipher {
    pub fn new(code: u16, secret: Vec<u8>) -> Result<Self, Error> {
        let hash = Hash::of(code);
        let key = hash.label(&secret, "key", &[], Suite::width(code));
        Ok(Self {
            suite: Suite::new(code, &key)?,
            code,
            hash,
            seed: hash.label(&secret, "iv", &[], 12),
            secret,
            count: 0,
        })
    }

    pub fn update(&mut self) -> Result<(), Error> {
        let secret = self
            .hash
            .label(&self.secret, "traffic upd", &[], self.hash.size());
        *self = Self::new(self.code, secret)?;
        Ok(())
    }

    fn nonce(&mut self) -> Vec<u8> {
        let mut nonce = self.seed.clone();
        for (slot, byte) in nonce[4..].iter_mut().zip(self.count.to_be_bytes()) {
            *slot ^= byte;
        }
        self.count += 1;
        nonce
    }

    pub fn seal(&mut self, kind: u8, body: &[u8]) -> Result<Vec<u8>, Error> {
        let mut inner = body.to_vec();
        inner.push(kind);
        let size = u16::try_from(inner.len() + TAG)
            .map_err(|_| Error::new("a record is too long to seal"))?;
        let mut record = vec![APPLICATION, 3, 3];
        record.extend_from_slice(&size.to_be_bytes());
        let nonce = self.nonce();
        let sealed = self
            .suite
            .seal(
                &nonce,
                Payload {
                    msg: &inner,
                    aad: &record,
                },
            )
            .ok_or_else(|| Error::new("cannot seal a record"))?;
        record.extend_from_slice(&sealed);
        Ok(record)
    }

    pub fn open(&mut self, header: &[u8], body: &[u8]) -> Result<(u8, Vec<u8>), Error> {
        let nonce = self.nonce();
        let mut inner = self
            .suite
            .open(
                &nonce,
                Payload {
                    msg: body,
                    aad: header,
                },
            )
            .ok_or_else(|| Error::new("a record from the node did not open"))?;
        while inner.last() == Some(&0) {
            inner.pop();
        }
        let kind = inner
            .pop()
            .ok_or_else(|| Error::new("a record from the node carried no type"))?;
        Ok((kind, inner))
    }
}
