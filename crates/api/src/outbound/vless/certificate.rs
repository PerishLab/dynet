use dynet_core::Error;
use hmac::{Hmac, Mac};
use sha2::Sha512;

const SEQUENCE: u8 = 0x30;
const BITS: u8 = 0x03;
const OBJECT: u8 = 0x06;
const VERSION: u8 = 0xa0;
const EDWARDS: [u8; 3] = [0x2b, 0x65, 0x70];

struct Reader<'a> {
    body: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self) -> Result<(u8, &'a [u8]), Error> {
        let tag = *self.body.first().ok_or_else(short)?;
        let first = *self.body.get(1).ok_or_else(short)?;
        let (size, skip) = match first {
            0..=0x7f => (usize::from(first), 2),
            0x81..=0x84 => {
                let count = usize::from(first & 0x7f);
                let digits = self.body.get(2..2 + count).ok_or_else(short)?;
                let size = digits
                    .iter()
                    .fold(0usize, |held, byte| (held << 8) | usize::from(*byte));
                (size, 2 + count)
            }
            _ => return Err(Error::new("a certificate carries an unreadable length")),
        };
        let value = self.body.get(skip..skip + size).ok_or_else(short)?;
        self.body = &self.body[skip + size..];
        Ok((tag, value))
    }

    fn expect(&mut self, wanted: u8) -> Result<&'a [u8], Error> {
        let (tag, value) = self.take()?;
        match tag == wanted {
            true => Ok(value),
            false => Err(Error::new(format!(
                "a certificate carries tag {tag:#04x} where {wanted:#04x} belongs"
            ))),
        }
    }
}

fn short() -> Error {
    Error::new("a certificate ends early")
}

pub struct Leaf {
    pub key: [u8; 32],
    pub signature: Vec<u8>,
}

pub fn read(der: &[u8]) -> Result<Leaf, Error> {
    let whole = Reader { body: der }.expect(SEQUENCE)?;
    let mut outer = Reader { body: whole };
    let signed = outer.expect(SEQUENCE)?;
    outer.expect(SEQUENCE)?;
    let signature = outer.expect(BITS)?.get(1..).ok_or_else(short)?.to_vec();
    let mut inner = Reader { body: signed };
    if inner.body.first() == Some(&VERSION) {
        inner.take()?;
    }
    for _ in 0..5 {
        inner.take()?;
    }
    let mut public = Reader {
        body: inner.expect(SEQUENCE)?,
    };
    let mut algorithm = Reader {
        body: public.expect(SEQUENCE)?,
    };
    if algorithm.expect(OBJECT)? != EDWARDS {
        return Err(Error::new("the node's certificate is not an Ed25519 key"));
    }
    let bits = public.expect(BITS)?;
    let key = bits
        .get(1..)
        .and_then(|value| <[u8; 32]>::try_from(value).ok())
        .ok_or_else(|| Error::new("the node's Ed25519 key is not thirty two bytes"))?;
    Ok(Leaf { key, signature })
}

pub fn vouch(der: &[u8], auth: &[u8]) -> Result<[u8; 32], Error> {
    let leaf = read(der)?;
    let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(auth).expect("any key length");
    mac.update(&leaf.key);
    mac.verify_slice(&leaf.signature).map_err(|_| {
        Error::new("the node answered with a certificate it holds no REALITY key for; refusing it")
    })?;
    Ok(leaf.key)
}
