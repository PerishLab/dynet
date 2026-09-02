use super::raw::random;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use dynet_core::Error;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

const CONTEXT: &str = "shadowsocks 2022 session subkey";
const ASKING: u8 = 0;
const ANSWERING: u8 = 1;
const SALT: usize = 16;
const TAG: usize = 16;
const SKEW: u64 = 30;
const CHAFF: usize = 512;

pub struct Cloak {
    salt: [u8; SALT],
    outward: Aes128Gcm,
    outgoing: u64,
}

pub struct Shroud {
    secret: [u8; SALT],
    salt: [u8; SALT],
    inward: Option<Aes128Gcm>,
    incoming: u64,
    spare: Option<Vec<u8>>,
}

pub fn veil(secret: [u8; SALT]) -> Result<(Cloak, Shroud), Error> {
    let drawn = random(SALT)?;
    let mut salt = [0u8; SALT];
    salt.copy_from_slice(&drawn);
    let cloak = Cloak {
        salt,
        outward: cipher(&secret, &salt),
        outgoing: 0,
    };
    let shroud = Shroud {
        secret,
        salt,
        inward: None,
        incoming: 0,
        spare: None,
    };
    Ok((cloak, shroud))
}

impl Cloak {
    pub fn greet(
        &mut self,
        out: &mut impl Write,
        seat: (&str, u16),
        payload: &[u8],
    ) -> Result<(), Error> {
        let mut asking = addressed(seat.0, seat.1)?;
        let chaff = match payload.is_empty() {
            true => random(CHAFF)?,
            false => Vec::new(),
        };
        let size = u16::try_from(chaff.len()).unwrap_or_default();
        asking.extend_from_slice(&size.to_be_bytes());
        asking.extend_from_slice(&chaff);
        asking.extend_from_slice(payload);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::new("the clock is behind the epoch"))?
            .as_secs();
        let mut fixed = vec![ASKING];
        fixed.extend_from_slice(&stamp.to_be_bytes());
        fixed.extend_from_slice(
            &u16::try_from(asking.len())
                .unwrap_or_default()
                .to_be_bytes(),
        );
        let mut request = self.salt.to_vec();
        request.extend_from_slice(&self.seal(&fixed)?);
        request.extend_from_slice(&self.seal(&asking)?);
        out.write_all(&request)
            .map_err(|error| Error::new(format!("cannot greet the exit: {error}")))
    }

    pub fn send(&mut self, out: &mut impl Write, body: &[u8]) -> Result<(), Error> {
        let length = u16::try_from(body.len()).map_err(|_| Error::new("a chunk is too long"))?;
        let mut framed = self.seal(&length.to_be_bytes())?;
        framed.extend_from_slice(&self.seal(body)?);
        out.write_all(&framed)
            .map_err(|error| Error::new(format!("cannot write to the exit: {error}")))
    }

    fn seal(&mut self, body: &[u8]) -> Result<Vec<u8>, Error> {
        let nonce = counted(self.outgoing);
        self.outgoing += 1;
        self.outward
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: body,
                    aad: b"",
                },
            )
            .map_err(|_| Error::new("cannot seal a chunk for the exit"))
    }
}

impl Shroud {
    pub fn receive(&mut self, inp: &mut impl Read) -> Result<Option<Vec<u8>>, Error> {
        if self.inward.is_none() {
            self.accept(inp)?;
        }
        if let Some(held) = self.spare.take() {
            return Ok(Some(held));
        }
        let Some(length) = self.take(inp, 2 + TAG)? else {
            return Ok(None);
        };
        let size = usize::from(u16::from_be_bytes([length[0], length[1]]));
        let Some(body) = self.take(inp, size + TAG)? else {
            return Ok(None);
        };
        Ok(Some(body))
    }

    fn accept(&mut self, inp: &mut impl Read) -> Result<(), Error> {
        let mut salt = [0u8; SALT];
        read(inp, &mut salt)?;
        self.inward = Some(cipher(&self.secret, &salt));
        let Some(fixed) = self.take(inp, 1 + 8 + SALT + 2 + TAG)? else {
            return Err(Error::new("the exit closed before answering"));
        };
        if fixed.first() != Some(&ANSWERING) {
            return Err(Error::new("the exit answered with the wrong header kind"));
        }
        if fixed.get(9..9 + SALT) != Some(&self.salt[..]) {
            return Err(Error::new(
                "the exit echoed a different session; the handshake was not accepted",
            ));
        }
        timely(&fixed[1..9])?;
        let told = usize::from(u16::from_be_bytes([fixed[9 + SALT], fixed[10 + SALT]]));
        if told > 0 {
            self.spare = self.take(inp, told + TAG)?;
        }
        Ok(())
    }

    fn take(&mut self, inp: &mut impl Read, size: usize) -> Result<Option<Vec<u8>>, Error> {
        let mut sealed = vec![0u8; size];
        if !read(inp, &mut sealed)? {
            return Ok(None);
        }
        let nonce = counted(self.incoming);
        self.incoming += 1;
        let inward = self
            .inward
            .as_ref()
            .ok_or_else(|| Error::new("the exit session is not open"))?;
        inward
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: b"",
                },
            )
            .map(Some)
            .map_err(|_| Error::new("the exit's answer did not open"))
    }
}

fn cipher(secret: &[u8; SALT], salt: &[u8; SALT]) -> Aes128Gcm {
    let mut seed = secret.to_vec();
    seed.extend_from_slice(salt);
    let drawn = blake3::derive_key(CONTEXT, &seed);
    Aes128Gcm::new(Key::<Aes128Gcm>::from_slice(&drawn[..SALT]))
}

fn counted(count: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&count.to_le_bytes());
    nonce
}

fn read(inp: &mut impl Read, room: &mut [u8]) -> Result<bool, Error> {
    match inp.read_exact(room) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(Error::new(format!("the exit stopped speaking: {error}"))),
    }
}

fn timely(stamp: &[u8]) -> Result<(), Error> {
    let mut held = [0u8; 8];
    held.copy_from_slice(stamp);
    let told = u64::from_be_bytes(held);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::new("the clock is behind the epoch"))?
        .as_secs();
    match now.abs_diff(told) <= SKEW {
        true => Ok(()),
        false => Err(Error::new("the exit answered with a stale timestamp")),
    }
}

fn addressed(host: &str, port: u16) -> Result<Vec<u8>, Error> {
    let mut told = match host.parse() {
        Ok(std::net::IpAddr::V4(plain)) => [vec![1u8], plain.octets().to_vec()].concat(),
        Ok(std::net::IpAddr::V6(plain)) => [vec![4u8], plain.octets().to_vec()].concat(),
        Err(_) => {
            let size = u8::try_from(host.len()).map_err(|_| Error::new("a name is too long"))?;
            [vec![3u8, size], host.as_bytes().to_vec()].concat()
        }
    };
    told.extend_from_slice(&port.to_be_bytes());
    Ok(told)
}
