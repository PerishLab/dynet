use super::blame;
use super::chunk::Chunk;
use super::kdf;
use super::raw::{checksum, parse, random};
use aes::Aes128;
use aes::cipher::BlockEncrypt;
use aes::cipher::KeyInit as Block;
use aes::cipher::generic_array::GenericArray;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use dynet_core::{Error, Fault};
use md5::{Digest as Legacy, Md5};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SALT: &[u8] = b"c48619fe-8f02-49e0-b9e9-edf763e17e21";
const SECURITY: u8 = 0x03;
const OPTION: u8 = 0x01;

pub struct Endpoint {
    host: String,
    port: u16,
    identity: [u8; 16],
}

pub struct Tunnel {
    stream: TcpStream,
    outbound: Chunk,
    inbound: Option<Chunk>,
    reply: [u8; 16],
    seed: [u8; 16],
    verify: u8,
    fault: Option<Fault>,
}

impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16, uuid: &str) -> Result<Self, Error> {
        Ok(Self {
            host: host.into(),
            port,
            identity: parse(uuid)?,
        })
    }

    fn command(&self) -> [u8; 16] {
        let mut hasher = Md5::new();
        hasher.update(self.identity);
        hasher.update(SALT);
        hasher.finalize().into()
    }
}

impl Tunnel {
    pub fn open(endpoint: &Endpoint, host: &str, port: u16) -> Result<Self, Error> {
        let mut tunnel = Self::dial(endpoint)?;
        tunnel.board(endpoint, host, port)?;
        Ok(tunnel)
    }

    pub fn dial(endpoint: &Endpoint) -> Result<Self, Error> {
        let stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .map_err(|error| Error::new(format!("cannot reach the node: {error}")))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(20))))
            .map_err(|error| Error::new(format!("cannot bound the node session: {error}")))?;
        Self::dress(stream)
    }

    pub fn board(&mut self, endpoint: &Endpoint, host: &str, port: u16) -> Result<(), Error> {
        let header = self.header(host, port);
        self.greet(endpoint, &header)
    }

    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    fn dress(stream: TcpStream) -> Result<Self, Error> {
        let secret = random(33)?;
        let mut key = [0u8; 16];
        let mut seed = [0u8; 16];
        key.copy_from_slice(&secret[..16]);
        seed.copy_from_slice(&secret[16..32]);
        Ok(Self {
            stream,
            outbound: Chunk::new(&key, seed),
            inbound: None,
            reply: key,
            seed,
            verify: secret[32],
            fault: None,
        })
    }

    fn header(&self, host: &str, port: u16) -> Vec<u8> {
        let mut header = vec![1u8];
        header.extend_from_slice(&self.seed);
        header.extend_from_slice(&self.reply);
        header.push(self.verify);
        header.push(OPTION);
        header.push(SECURITY);
        header.push(0);
        header.push(1);
        header.extend_from_slice(&port.to_be_bytes());
        header.push(2);
        header.push(u8::try_from(host.len()).unwrap_or_default());
        header.extend_from_slice(host.as_bytes());
        let mark = kdf::fingerprint(&header);
        header.extend_from_slice(&mark);
        header
    }

    fn greet(&mut self, endpoint: &Endpoint, header: &[u8]) -> Result<(), Error> {
        let command = endpoint.command();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::new("the clock is behind the epoch"))?
            .as_secs();
        let auth = identify(&command, stamp)?;
        let nonce = random(8)?;
        let mut request = auth.to_vec();
        request.extend_from_slice(&guard(&command, &auth, &nonce, header.len())?);
        request.extend_from_slice(&nonce);
        request.extend_from_slice(&conceal(&command, &auth, &nonce, header)?);
        self.stream
            .write_all(&request)
            .map_err(|error| Error::new(format!("cannot greet the node: {error}")))
    }

    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        self.outbound.send(&mut self.stream, body)
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

fn identify(command: &[u8; 16], stamp: u64) -> Result<[u8; 16], Error> {
    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&stamp.to_be_bytes());
    block[8..12].copy_from_slice(&random(4)?);
    let mark = checksum(&block[..12]);
    block[12..].copy_from_slice(&mark);
    let key = kdf::shorten(command, &[b"AES Auth ID Encryption"]);
    let cipher = <Aes128 as Block>::new(GenericArray::from_slice(&key));
    let mut sealed = GenericArray::clone_from_slice(&block);
    cipher.encrypt_block(&mut sealed);
    Ok(sealed.into())
}

fn guard(command: &[u8; 16], auth: &[u8; 16], nonce: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    let length = u16::try_from(size).map_err(|_| Error::new("the header is too long"))?;
    seal(
        &kdf::shorten(command, &[b"VMess Header AEAD Key_Length", auth, nonce]),
        &kdf::derive(command, &[b"VMess Header AEAD Nonce_Length", auth, nonce]),
        auth,
        &length.to_be_bytes(),
    )
}

fn conceal(
    command: &[u8; 16],
    auth: &[u8; 16],
    nonce: &[u8],
    header: &[u8],
) -> Result<Vec<u8>, Error> {
    seal(
        &kdf::shorten(command, &[b"VMess Header AEAD Key", auth, nonce]),
        &kdf::derive(command, &[b"VMess Header AEAD Nonce", auth, nonce]),
        auth,
        header,
    )
}

fn seal(key: &[u8; 16], seed: &[u8; 32], auth: &[u8; 16], body: &[u8]) -> Result<Vec<u8>, Error> {
    let cipher = Aes128Gcm::new(Key::<Aes128Gcm>::from_slice(key));
    cipher
        .encrypt(
            Nonce::from_slice(&seed[..12]),
            Payload {
                msg: body,
                aad: auth,
            },
        )
        .map_err(|_| Error::new("cannot seal the request header"))
}
