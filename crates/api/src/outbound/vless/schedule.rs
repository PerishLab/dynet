use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha384};

const WIDE: u16 = 0x1302;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hash {
    Short,
    Long,
}

impl Hash {
    pub fn of(suite: u16) -> Self {
        match suite {
            WIDE => Self::Long,
            _ => Self::Short,
        }
    }

    pub fn size(self) -> usize {
        match self {
            Self::Short => 32,
            Self::Long => 48,
        }
    }

    pub fn digest(self, body: &[u8]) -> Vec<u8> {
        match self {
            Self::Short => Sha256::digest(body).to_vec(),
            Self::Long => Sha384::digest(body).to_vec(),
        }
    }

    pub fn mac(self, key: &[u8], body: &[u8]) -> Vec<u8> {
        match self {
            Self::Short => {
                let mut held = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
                held.update(body);
                held.finalize().into_bytes().to_vec()
            }
            Self::Long => {
                let mut held = <Hmac<Sha384> as Mac>::new_from_slice(key).expect("any key length");
                held.update(body);
                held.finalize().into_bytes().to_vec()
            }
        }
    }

    pub fn extract(self, salt: &[u8], keying: &[u8]) -> Vec<u8> {
        match salt.is_empty() {
            true => self.mac(&vec![0u8; self.size()], keying),
            false => self.mac(salt, keying),
        }
    }

    pub fn expand(self, secret: &[u8], info: &[u8], size: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut last = Vec::new();
        let mut count = 1u8;
        while out.len() < size {
            let mut block = last.clone();
            block.extend_from_slice(info);
            block.push(count);
            last = self.mac(secret, &block);
            out.extend_from_slice(&last);
            count = count.wrapping_add(1);
        }
        out.truncate(size);
        out
    }

    pub fn label(self, secret: &[u8], name: &str, context: &[u8], size: usize) -> Vec<u8> {
        let full = format!("tls13 {name}");
        let mut info = u16::try_from(size)
            .unwrap_or_default()
            .to_be_bytes()
            .to_vec();
        info.push(u8::try_from(full.len()).unwrap_or_default());
        info.extend_from_slice(full.as_bytes());
        info.push(u8::try_from(context.len()).unwrap_or_default());
        info.extend_from_slice(context);
        self.expand(secret, &info, size)
    }

    pub fn derive(self, secret: &[u8], name: &str, transcript: &[u8]) -> Vec<u8> {
        self.label(secret, name, &self.digest(transcript), self.size())
    }

    pub fn finish(self, secret: &[u8], transcript: &[u8]) -> Vec<u8> {
        let key = self.label(secret, "finished", &[], self.size());
        self.mac(&key, &self.digest(transcript))
    }
}

pub struct Ladder {
    pub hash: Hash,
    pub client: Vec<u8>,
    pub server: Vec<u8>,
    master: Vec<u8>,
}

impl Ladder {
    pub fn climb(hash: Hash, shared: &[u8], transcript: &[u8]) -> Self {
        let early = hash.extract(&[], &vec![0u8; hash.size()]);
        let salt = hash.derive(&early, "derived", &[]);
        let handshake = hash.extract(&salt, shared);
        let next = hash.derive(&handshake, "derived", &[]);
        Self {
            hash,
            client: hash.derive(&handshake, "c hs traffic", transcript),
            server: hash.derive(&handshake, "s hs traffic", transcript),
            master: hash.extract(&next, &vec![0u8; hash.size()]),
        }
    }

    pub fn settle(&self, transcript: &[u8]) -> (Vec<u8>, Vec<u8>) {
        (
            self.hash.derive(&self.master, "c ap traffic", transcript),
            self.hash.derive(&self.master, "s ap traffic", transcript),
        )
    }
}
