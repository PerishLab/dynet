use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use dynet_core::Error;
use std::io::{Read, Write};

pub struct Chunk {
    cipher: Aes128Gcm,
    seed: [u8; 16],
    count: u16,
}

impl Chunk {
    pub fn new(key: &[u8; 16], seed: [u8; 16]) -> Self {
        Self {
            cipher: Aes128Gcm::new(Key::<Aes128Gcm>::from_slice(key)),
            seed,
            count: 0,
        }
    }

    fn nonce(&mut self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..2].copy_from_slice(&self.count.to_be_bytes());
        nonce[2..].copy_from_slice(&self.seed[2..12]);
        self.count = self.count.wrapping_add(1);
        nonce
    }

    pub fn send(&mut self, stream: &mut impl Write, body: &[u8]) -> Result<(), Error> {
        let nonce = self.nonce();
        let sealed = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: body,
                    aad: b"",
                },
            )
            .map_err(|_| Error::new("cannot seal a chunk"))?;
        let length = u16::try_from(sealed.len()).map_err(|_| Error::new("chunk too long"))?;
        stream
            .write_all(&length.to_be_bytes())
            .and_then(|()| stream.write_all(&sealed))
            .map_err(|error| Error::new(format!("cannot write a chunk: {error}")))
    }

    pub fn receive(&mut self, stream: &mut impl Read) -> Result<Option<Vec<u8>>, Error> {
        let mut header = [0u8; 2];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(Error::new(format!("cannot read a chunk: {error}"))),
        }
        let length = usize::from(u16::from_be_bytes(header));
        if length == 0 {
            return Ok(None);
        }
        let mut sealed = vec![0u8; length];
        stream
            .read_exact(&mut sealed)
            .map_err(|error| Error::new(format!("cannot read a chunk body: {error}")))?;
        let nonce = self.nonce();
        self.cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: b"",
                },
            )
            .map(Some)
            .map_err(|_| Error::new("a chunk did not open; the session key or counter is wrong"))
    }
}
