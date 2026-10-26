use super::Reality;
use super::hello::{self, CURVE, HYBRID, Shares};
use super::message::{Answer, certified, framed, pair, split};
use super::record::{ALERT, APPLICATION, CHANGE, Cipher, HANDSHAKE};
use super::schedule::{Hash, Ladder};
use super::session::{Reader, Session, Wire, Writer};
use crate::outbound::raw::random;
use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use dynet_core::Error;
use ed25519_dalek::{Signature, VerifyingKey};
use ml_kem::kem::Decapsulate;
use ml_kem::{Ciphertext, EncodedSizeUser, KemCore, MlKem768};
use rand_core::OsRng;
use std::io::Write;
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use x25519_dalek::{PublicKey, StaticSecret};

type Lattice = <MlKem768 as KemCore>::DecapsulationKey;

const RELEASE: [u8; 3] = [25, 9, 11];
const BUDGET: Duration = Duration::from_secs(5);
const MARGIN: &[u8] = b"TLS 1.3, server CertificateVerify";
const SCHEME: u16 = 0x0807;
const SERVER: u8 = 2;
const EXTENSIONS: u8 = 8;
const CERTIFICATE: u8 = 11;
const COMPRESSED: u8 = 25;
const VERIFY: u8 = 15;
const FINISHED: u8 = 20;

struct Flight<'a> {
    transcript: Vec<u8>,
    key: Option<[u8; 32]>,
    auth: &'a [u8],
    ladder: &'a Ladder,
}

pub fn shake(stream: TcpStream, reality: &Reality) -> Result<Session, Error> {
    stream
        .set_read_timeout(Some(BUDGET))
        .map_err(|error| Error::new(format!("cannot bound the handshake: {error}")))?;
    let curve = StaticSecret::random_from_rng(OsRng);
    let (lattice, encoded) = MlKem768::generate(&mut OsRng);
    let shares = Shares {
        curve: PublicKey::from(&curve).to_bytes(),
        lattice: encoded.as_bytes().to_vec(),
    };
    let seed: [u8; 32] = random(32)?
        .try_into()
        .map_err(|_| Error::new("randomness came short"))?;
    let mut message = hello::build(&seed, &reality.name, &shares)?;
    let auth = authorize(&curve, reality, &seed, &mut message)?;
    let listening = stream
        .try_clone()
        .map_err(|error| Error::new(format!("cannot halve the node session: {error}")))?;
    let mut wire = Wire::new(listening);
    let mut record = vec![HANDSHAKE, 3, 1];
    hello::extend(&mut record, &message);
    (&stream)
        .write_all(&record)
        .map_err(|error| Error::new(format!("cannot greet the node: {error}")))?;
    let (greeting, answer) = greeting(&mut wire)?;
    message.extend_from_slice(&greeting);
    let shared = agree(&curve, &lattice, &answer)?;
    let hash = Hash::of(answer.suite);
    let ladder = Ladder::climb(hash, &shared, &message);
    let mut flight = Flight {
        transcript: message,
        key: None,
        auth: &auth,
        ladder: &ladder,
    };
    flight.fly(&mut wire, Cipher::new(answer.suite, ladder.server.clone())?)?;
    finish(wire, stream, &flight, answer.suite)
}

fn finish(wire: Wire, stream: TcpStream, flight: &Flight, suite: u16) -> Result<Session, Error> {
    let ladder = flight.ladder;
    let (client, server) = ladder.settle(&flight.transcript);
    let verified = ladder.hash.finish(&ladder.client, &flight.transcript);
    let mut sealer = Cipher::new(suite, ladder.client.clone())?;
    let mut out = vec![CHANGE, 3, 3, 0, 1, 1];
    out.extend_from_slice(&sealer.seal(HANDSHAKE, &framed(FINISHED, &verified))?);
    let mut writer = Writer::new(stream, Cipher::new(suite, client)?);
    writer.raw(&out)?;
    Ok(Session {
        reader: Reader::new(wire, Cipher::new(suite, server)?),
        writer,
    })
}

pub fn authorize(
    curve: &StaticSecret,
    reality: &Reality,
    seed: &[u8; 32],
    message: &mut [u8],
) -> Result<Vec<u8>, Error> {
    let shared = curve.diffie_hellman(&PublicKey::from(reality.key));
    let hash = Hash::Short;
    let auth = hash.expand(
        &hash.extract(&seed[..20], shared.as_bytes()),
        b"REALITY",
        32,
    );
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::new("the clock is behind the epoch"))?
        .as_secs();
    let mut plain = [0u8; 16];
    plain[..3].copy_from_slice(&RELEASE);
    plain[4..8].copy_from_slice(&u32::try_from(stamp).unwrap_or_default().to_be_bytes());
    plain[8..].copy_from_slice(&reality.short);
    let cipher = Aes256Gcm::new_from_slice(&auth)
        .map_err(|_| Error::new("the REALITY key has the wrong length"))?;
    let sealed = cipher
        .encrypt(
            seed[20..].into(),
            Payload {
                msg: &plain,
                aad: message,
            },
        )
        .map_err(|_| Error::new("cannot seal the REALITY session"))?;
    message[39..71].copy_from_slice(&sealed);
    Ok(auth)
}

fn greeting(wire: &mut Wire) -> Result<(Vec<u8>, Answer), Error> {
    let mut pending = Vec::new();
    loop {
        if let Some((kind, message)) = split(&mut pending) {
            if kind != SERVER {
                return Err(Error::new(format!("the node answered with message {kind}")));
            }
            let answer = Answer::read(&message[4..])?;
            return Ok((message, answer));
        }
        let record = wire
            .next()?
            .ok_or_else(|| Error::new("the node closed before it answered the hello"))?;
        match record.kind {
            HANDSHAKE => pending.extend_from_slice(&record.body),
            ALERT => {
                return Err(Error::new(format!(
                    "the node refused the hello with alert {:?}",
                    record.body
                )));
            }
            other => return Err(Error::new(format!("the node answered with record {other}"))),
        }
    }
}

fn agree(curve: &StaticSecret, lattice: &Lattice, answer: &Answer) -> Result<Vec<u8>, Error> {
    let ragged = || Error::new("the node's key share is ragged");
    let (sealed, peer) = match answer.group {
        HYBRID if answer.share.len() == 1120 => answer.share.split_at(1088),
        CURVE if answer.share.len() == 32 => answer.share.split_at(0),
        other => return Err(Error::new(format!("the node chose group {other:#06x}"))),
    };
    let peer: [u8; 32] = peer.try_into().map_err(|_| ragged())?;
    let mut shared = Vec::new();
    if !sealed.is_empty() {
        let sealed = Ciphertext::<MlKem768>::try_from(sealed).map_err(|_| ragged())?;
        let secret = lattice.decapsulate(&sealed).map_err(|()| ragged())?;
        shared.extend_from_slice(&secret);
    }
    shared.extend_from_slice(curve.diffie_hellman(&PublicKey::from(peer)).as_bytes());
    Ok(shared)
}

impl Flight<'_> {
    fn fly(&mut self, wire: &mut Wire, mut opener: Cipher) -> Result<(), Error> {
        let mut pending = Vec::new();
        loop {
            while let Some((kind, message)) = split(&mut pending) {
                if self.absorb(kind, &message)? {
                    return Ok(());
                }
            }
            let record = wire
                .next()?
                .ok_or_else(|| Error::new("the node closed inside the handshake"))?;
            match record.kind {
                CHANGE => continue,
                APPLICATION => {}
                other => return Err(Error::new(format!("the node sent record {other} early"))),
            }
            let (kind, body) = opener.open(&record.header, &record.body)?;
            if kind != HANDSHAKE {
                return Err(Error::new(format!(
                    "the node sent {kind} inside the handshake"
                )));
            }
            pending.extend_from_slice(&body);
        }
    }

    fn absorb(&mut self, kind: u8, message: &[u8]) -> Result<bool, Error> {
        let body = &message[4..];
        match kind {
            EXTENSIONS => {}
            CERTIFICATE => self.key = Some(certified(body, self.auth)?),
            VERIFY => self.verify(body)?,
            FINISHED => {
                let hash = self.ladder.hash;
                if hash.finish(&self.ladder.server, &self.transcript) != body {
                    return Err(Error::new("the node's handshake did not finish cleanly"));
                }
                self.transcript.extend_from_slice(message);
                return Ok(true);
            }
            COMPRESSED => return Err(Error::new("the node compressed its certificate")),
            other => return Err(Error::new(format!("the node sent message {other}"))),
        }
        self.transcript.extend_from_slice(message);
        Ok(false)
    }

    fn verify(&self, body: &[u8]) -> Result<(), Error> {
        let refused = || Error::new("the node's certificate signature does not hold");
        let key = self
            .key
            .ok_or_else(|| Error::new("the node signed before it named a certificate"))?;
        if pair(body, 0) != Some(SCHEME) {
            return Err(Error::new(
                "the node signed with a scheme other than Ed25519",
            ));
        }
        let signature =
            Signature::from_slice(body.get(4..).ok_or_else(refused)?).map_err(|_| refused())?;
        let mut content = vec![0x20u8; 64];
        content.extend_from_slice(MARGIN);
        content.push(0);
        content.extend_from_slice(&self.ladder.hash.digest(&self.transcript));
        VerifyingKey::from_bytes(&key)
            .and_then(|held| held.verify_strict(&content, &signature))
            .map_err(|_| refused())
    }
}
