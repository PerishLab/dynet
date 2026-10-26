use super::certificate;
use dynet_core::Error;

const RETRY: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91,
    0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

pub struct Answer {
    pub suite: u16,
    pub group: u16,
    pub share: Vec<u8>,
}

impl Answer {
    pub fn read(body: &[u8]) -> Result<Self, Error> {
        let ragged = || Error::new("the node's hello is ragged");
        if body.get(2..34) == Some(&RETRY) {
            return Err(Error::new("the node asked for another hello"));
        }
        let skip = usize::from(*body.get(34).ok_or_else(ragged)?);
        let mut cursor = 35 + skip;
        let suite = pair(body, cursor).ok_or_else(ragged)?;
        cursor += 3;
        let total = usize::from(pair(body, cursor).ok_or_else(ragged)?);
        let mut rest = body
            .get(cursor + 2..cursor + 2 + total)
            .ok_or_else(ragged)?;
        let mut answer = Self {
            suite,
            group: 0,
            share: Vec::new(),
        };
        let mut modern = false;
        while rest.len() >= 4 {
            let kind = pair(rest, 0).ok_or_else(ragged)?;
            let size = usize::from(pair(rest, 2).ok_or_else(ragged)?);
            let data = rest.get(4..4 + size).ok_or_else(ragged)?;
            match kind {
                0x0033 => {
                    answer.group = pair(data, 0).ok_or_else(ragged)?;
                    answer.share = data.get(4..).ok_or_else(ragged)?.to_vec();
                }
                0x002b => modern = pair(data, 0) == Some(0x0304),
                _ => {}
            }
            rest = &rest[4 + size..];
        }
        match modern {
            true => Ok(answer),
            false => Err(Error::new("the node did not speak TLS 1.3")),
        }
    }
}

pub fn certified(body: &[u8], auth: &[u8]) -> Result<[u8; 32], Error> {
    let ragged = || Error::new("the node's certificate message is ragged");
    let skip = usize::from(*body.first().ok_or_else(ragged)?);
    let cursor = 1 + skip + 3;
    let size = triple(body, cursor).ok_or_else(ragged)?;
    let der = body.get(cursor + 3..cursor + 3 + size).ok_or_else(ragged)?;
    certificate::vouch(der, auth)
}

pub fn split(pending: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    let size = triple(pending, 1)?;
    if pending.len() < 4 + size {
        return None;
    }
    let message: Vec<u8> = pending.drain(..4 + size).collect();
    Some((message[0], message))
}

pub fn framed(kind: u8, body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(body.len()).unwrap_or_default().to_be_bytes();
    let mut out = vec![kind];
    out.extend_from_slice(&size[1..]);
    out.extend_from_slice(body);
    out
}

pub fn pair(body: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]))
}

pub fn triple(body: &[u8], at: usize) -> Option<usize> {
    let held = body.get(at..at + 3)?;
    Some(usize::from(held[0]) << 16 | usize::from(held[1]) << 8 | usize::from(held[2]))
}
