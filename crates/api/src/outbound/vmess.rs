use super::chunk::Chunk;
use super::kdf;
use super::parts::{Egress, Ingress};
use super::raw::{checksum, parse, random};
use aes::Aes128;
use aes::cipher::BlockEncrypt;
use aes::cipher::KeyInit as Block;
use aes::cipher::generic_array::GenericArray;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use dynet_core::{Error, Fault};
use md5::{Digest as Legacy, Md5};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SALT: &[u8] = b"c48619fe-8f02-49e0-b9e9-edf763e17e21";
const NEAR: Duration = Duration::from_secs(1);
const BRIEF: Duration = Duration::from_secs(4);
const PATIENT: Duration = Duration::from_secs(20);
const SECURITY: u8 = 0x03;
const OPTION: u8 = 0x01;
const STREAM: u8 = 0x01;
const DATAGRAM: u8 = 0x02;

pub struct Endpoint {
    host: String,
    port: u16,
    identity: [u8; 16],
    mark: u32,
}

pub struct Tunnel {
    egress: Egress,
    ingress: Ingress,
    reply: [u8; 16],
    seed: [u8; 16],
    verify: u8,
}

impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16, uuid: &str, mark: u32) -> Result<Self, Error> {
        Ok(Self {
            host: host.into(),
            port,
            identity: parse(uuid)?,
            mark,
        })
    }

    fn seat(&self) -> Result<SocketAddr, Error> {
        let named = format!("{}:{}", self.host, self.port);
        if let Ok(seat) = named.parse() {
            return Ok(seat);
        }
        named
            .to_socket_addrs()
            .map_err(|error| Error::new(format!("cannot place the node: {error}")))?
            .next()
            .ok_or_else(|| Error::new(format!("{} names no address", self.host)))
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
        let seat = endpoint.seat()?;
        let stream = crate::outbound::reach(seat, endpoint.mark, NEAR)?;
        stream
            .set_read_timeout(Some(BRIEF))
            .and_then(|()| stream.set_write_timeout(Some(PATIENT)))
            .map_err(|error| Error::new(format!("cannot bound the node session: {error}")))?;
        Self::dress(stream)
    }

    pub fn board(&mut self, endpoint: &Endpoint, host: &str, port: u16) -> Result<(), Error> {
        let header = self.header(host, port, STREAM);
        self.greet(endpoint, &header)
    }

    pub fn bear(&mut self, endpoint: &Endpoint, host: &str, port: u16) -> Result<(), Error> {
        let header = self.header(host, port, DATAGRAM);
        self.greet(endpoint, &header)
    }

    pub fn split(self) -> (Egress, Ingress) {
        (self.egress, self.ingress)
    }

    pub fn send(&mut self, body: &[u8]) -> Result<(), Error> {
        self.egress.send(body)
    }

    pub fn receive(&mut self) -> Result<Option<Vec<u8>>, Error> {
        self.ingress.receive()
    }

    pub fn fault(&self) -> Option<Fault> {
        self.ingress.fault()
    }

    fn dress(stream: TcpStream) -> Result<Self, Error> {
        let secret = random(33)?;
        let mut key = [0u8; 16];
        let mut seed = [0u8; 16];
        key.copy_from_slice(&secret[..16]);
        seed.copy_from_slice(&secret[16..32]);
        let listening = stream
            .try_clone()
            .map_err(|error| Error::new(format!("cannot halve the node session: {error}")))?;
        Ok(Self {
            egress: Egress::new(stream, Chunk::new(&key, seed)),
            ingress: Ingress::new(listening, key, seed, secret[32]),
            reply: key,
            seed,
            verify: secret[32],
        })
    }

    fn header(&self, host: &str, port: u16, command: u8) -> Vec<u8> {
        let mut header = vec![1u8];
        header.extend_from_slice(&self.seed);
        header.extend_from_slice(&self.reply);
        header.push(self.verify);
        header.push(OPTION);
        header.push(SECURITY);
        header.push(0);
        header.push(command);
        header.extend_from_slice(&port.to_be_bytes());
        header.extend_from_slice(&addressed(host));
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
        self.egress.greet(&request)
    }
}

fn addressed(host: &str) -> Vec<u8> {
    match host.parse() {
        Ok(IpAddr::V4(plain)) => [vec![1u8], plain.octets().to_vec()].concat(),
        Ok(IpAddr::V6(plain)) => [vec![3u8], plain.octets().to_vec()].concat(),
        Err(_) => [
            vec![2u8, u8::try_from(host.len()).unwrap_or_default()],
            host.as_bytes().to_vec(),
        ]
        .concat(),
    }
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
