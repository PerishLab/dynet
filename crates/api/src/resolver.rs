use dynet_core::Error;
use std::net::Ipv4Addr;

const ADDRESS: u16 = 1;
const MARK: u16 = 0x5151;
const ROUNDS: usize = 3;
const BRIEF: std::time::Duration = std::time::Duration::from_secs(3);
pub const QUAD: u16 = 28;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Answer {
    pub address: Ipv4Addr,
    pub life: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub name: String,
    pub kind: u16,
    pub span: usize,
}

pub struct Packet<'a> {
    body: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn new(body: &'a [u8]) -> Self {
        Self { body }
    }

    pub fn asked(&self) -> Result<Query, Error> {
        let packet = self.body;
        if packet.len() < 17 {
            return Err(Error::new("a query is too short to carry a question"));
        }
        if u16::from_be_bytes([packet[4], packet[5]]) != 1 {
            return Err(Error::new("a query must carry exactly one question"));
        }
        let mut labels = Vec::new();
        let mut cursor = 12;
        loop {
            let size = usize::from(*packet.get(cursor).ok_or_else(ragged)?);
            if size & 0xc0 == 0xc0 {
                return Err(Error::new("a question must not be compressed"));
            }
            cursor += 1;
            if size == 0 {
                break;
            }
            let label = packet.get(cursor..cursor + size).ok_or_else(ragged)?;
            labels.push(String::from_utf8_lossy(label).into_owned());
            cursor += size;
        }
        let kind = packet.get(cursor..cursor + 2).ok_or_else(ragged)?;
        Ok(Query {
            name: labels.join("."),
            kind: u16::from_be_bytes([kind[0], kind[1]]),
            span: cursor + 4,
        })
    }

    pub fn barren(&self, query: &Query) -> Vec<u8> {
        let packet = self.body;
        let mut answer = packet[..query.span.min(packet.len())].to_vec();
        if answer.len() < 12 {
            return answer;
        }
        answer[2] = 0x81;
        answer[3] = 0x80;
        answer[6..12].fill(0);
        answer[4] = 0;
        answer[5] = 1;
        answer
    }

    pub fn frame(&self) -> Result<Vec<u8>, Error> {
        let length =
            u16::try_from(self.body.len()).map_err(|_| Error::new("the query is too long"))?;
        let mut framed = length.to_be_bytes().to_vec();
        framed.extend_from_slice(self.body);
        Ok(framed)
    }

    pub fn relay(&self, upstream: &str) -> Result<Vec<u8>, Error> {
        let socket = std::net::UdpSocket::bind(("0.0.0.0", 0))
            .map_err(|error| Error::new(format!("cannot open a relaying socket: {error}")))?;
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(8)))
            .map_err(|error| Error::new(format!("cannot bound a relay: {error}")))?;
        socket
            .send_to(self.body, (upstream, 53))
            .map_err(|error| Error::new(format!("cannot pass a question upstream: {error}")))?;
        let mut room = [0u8; 1500];
        let size = socket
            .recv(&mut room)
            .map_err(|error| Error::new(format!("the upstream said nothing: {error}")))?;
        Ok(room[..size].to_vec())
    }
}

pub fn renew(body: &[u8], mark: u16, aged: u32) -> Result<Vec<u8>, Error> {
    let mut fresh = body.to_vec();
    if fresh.len() < 12 {
        return Err(Error::new("a remembered answer lost its header"));
    }
    fresh[..2].copy_from_slice(&mark.to_be_bytes());
    let count = usize::from(u16::from_be_bytes([fresh[6], fresh[7]]));
    let mut cursor = skip(&fresh, 12)? + 4;
    for _ in 0..count {
        cursor = skip(&fresh, cursor)?;
        if cursor + 10 > fresh.len() {
            break;
        }
        let held = u32::from_be_bytes([
            fresh[cursor + 4],
            fresh[cursor + 5],
            fresh[cursor + 6],
            fresh[cursor + 7],
        ]);
        let left = held.saturating_sub(aged).max(1);
        fresh[cursor + 4..cursor + 8].copy_from_slice(&left.to_be_bytes());
        let size = usize::from(u16::from_be_bytes([fresh[cursor + 8], fresh[cursor + 9]]));
        cursor += 10 + size;
    }
    Ok(fresh)
}

fn ragged() -> Error {
    Error::new("a question ran past the query")
}

pub fn ask(name: &str, mark: u16) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    body.extend_from_slice(&mark.to_be_bytes());
    body.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        let size = u8::try_from(label.len()).map_err(|_| Error::new("a label is too long"))?;
        body.push(size);
        body.extend_from_slice(label.as_bytes());
    }
    body.push(0);
    body.extend_from_slice(&ADDRESS.to_be_bytes());
    body.extend_from_slice(&[0, 1]);
    let length = u16::try_from(body.len()).map_err(|_| Error::new("the query is too long"))?;
    let mut framed = length.to_be_bytes().to_vec();
    framed.extend_from_slice(&body);
    Ok(framed)
}

pub fn locate(name: &str, upstream: &str) -> Result<Answer, Error> {
    let mut spoken = Error::new(format!("{name} was never asked"));
    for round in 0..ROUNDS {
        let mark = MARK.wrapping_add(u16::try_from(round).unwrap_or_default());
        match attempt(name, upstream, mark) {
            Ok(answer) => return Ok(answer),
            Err(error) => spoken = error,
        }
    }
    Err(spoken)
}

fn attempt(name: &str, upstream: &str, mark: u16) -> Result<Answer, Error> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0))
        .map_err(|error| Error::new(format!("cannot open a resolving socket: {error}")))?;
    socket
        .set_read_timeout(Some(BRIEF))
        .map_err(|error| Error::new(format!("cannot bound a lookup: {error}")))?;
    let framed = ask(name, mark)?;
    socket
        .send_to(&framed[2..], (upstream, 53))
        .map_err(|error| Error::new(format!("cannot ask about {name}: {error}")))?;
    let mut room = [0u8; 1500];
    let size = socket
        .recv(&mut room)
        .map_err(|error| Error::new(format!("no answer about {name}: {error}")))?;
    read(&room[..size])?
        .first()
        .copied()
        .ok_or_else(|| Error::new(format!("{name} resolved to no address")))
}

pub fn read(body: &[u8]) -> Result<Vec<Answer>, Error> {
    if body.len() < 12 {
        return Err(Error::new("the answer is too short to carry a header"));
    }
    let count = usize::from(u16::from_be_bytes([body[6], body[7]]));
    let mut cursor = 12;
    cursor = skip(body, cursor)? + 4;
    let mut found = Vec::new();
    for _ in 0..count {
        cursor = skip(body, cursor)?;
        if cursor + 10 > body.len() {
            break;
        }
        let kind = u16::from_be_bytes([body[cursor], body[cursor + 1]]);
        let life = u32::from_be_bytes([
            body[cursor + 4],
            body[cursor + 5],
            body[cursor + 6],
            body[cursor + 7],
        ]);
        let size = usize::from(u16::from_be_bytes([body[cursor + 8], body[cursor + 9]]));
        cursor += 10;
        if kind == ADDRESS && size == 4 && cursor + 4 <= body.len() {
            found.push(Answer {
                address: Ipv4Addr::new(
                    body[cursor],
                    body[cursor + 1],
                    body[cursor + 2],
                    body[cursor + 3],
                ),
                life,
            });
        }
        cursor += size;
    }
    Ok(found)
}

fn skip(body: &[u8], mut cursor: usize) -> Result<usize, Error> {
    loop {
        let Some(size) = body.get(cursor) else {
            return Err(Error::new("a name ran past the answer"));
        };
        if size & 0xc0 == 0xc0 {
            return Ok(cursor + 2);
        }
        if *size == 0 {
            return Ok(cursor + 1);
        }
        cursor += usize::from(*size) + 1;
    }
}
