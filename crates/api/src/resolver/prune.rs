use super::skip;
use dynet_core::Error;
use std::net::Ipv4Addr;

const ADDRESS: u16 = 1;
const ALIAS: u16 = 5;
const DEPTH: usize = 16;

struct Record {
    owner: Vec<u8>,
    kind: u16,
    rest: Vec<u8>,
    data: Vec<u8>,
}

pub fn prune(body: &[u8], spared: impl Fn(Ipv4Addr) -> bool) -> Result<Option<Vec<u8>>, Error> {
    if body.len() < 12 {
        return Err(Error::new("the answer is too short to carry a header"));
    }
    let count = usize::from(u16::from_be_bytes([body[6], body[7]]));
    let asked = skip(body, 12)? + 4;
    let mut cursor = asked;
    let mut kept = Vec::new();
    let mut dropped = 0;
    for _ in 0..count {
        let (record, next) = record(body, cursor)?;
        cursor = next;
        let address = (record.kind == ADDRESS && record.data.len() == 4).then(|| {
            Ipv4Addr::new(
                record.data[0],
                record.data[1],
                record.data[2],
                record.data[3],
            )
        });
        match address.is_some_and(&spared) {
            true => dropped += 1,
            false => kept.push(record),
        }
    }
    if dropped == 0 {
        return Ok(None);
    }
    let mut pruned = body.get(..asked).ok_or_else(ragged)?.to_vec();
    let total = u16::try_from(kept.len()).map_err(|_| ragged())?;
    pruned[6..8].copy_from_slice(&total.to_be_bytes());
    pruned[8..12].fill(0);
    for record in kept {
        pruned.extend_from_slice(&record.owner);
        pruned.extend_from_slice(&record.kind.to_be_bytes());
        pruned.extend_from_slice(&record.rest);
        let size = u16::try_from(record.data.len()).map_err(|_| ragged())?;
        pruned.extend_from_slice(&size.to_be_bytes());
        pruned.extend_from_slice(&record.data);
    }
    Ok(Some(pruned))
}

fn record(body: &[u8], cursor: usize) -> Result<(Record, usize), Error> {
    let owner = spell(body, cursor)?;
    let cursor = skip(body, cursor)?;
    let fixed = body.get(cursor..cursor + 10).ok_or_else(ragged)?;
    let kind = u16::from_be_bytes([fixed[0], fixed[1]]);
    let size = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
    let start = cursor + 10;
    let raw = body.get(start..start + size).ok_or_else(ragged)?;
    let data = match kind {
        ALIAS => spell(body, start)?,
        _ => raw.to_vec(),
    };
    let record = Record {
        owner,
        kind,
        rest: fixed[2..8].to_vec(),
        data,
    };
    Ok((record, start + size))
}

fn spell(body: &[u8], mut cursor: usize) -> Result<Vec<u8>, Error> {
    let mut spelled = Vec::new();
    for _ in 0..DEPTH {
        loop {
            let size = *body.get(cursor).ok_or_else(ragged)?;
            if size & 0xc0 == 0xc0 {
                let low = *body.get(cursor + 1).ok_or_else(ragged)?;
                cursor = usize::from(u16::from_be_bytes([size & 0x3f, low]));
                break;
            }
            let end = cursor + 1 + usize::from(size);
            spelled.extend_from_slice(body.get(cursor..end).ok_or_else(ragged)?);
            if size == 0 {
                return Ok(spelled);
            }
            cursor = end;
        }
    }
    Err(Error::new("an answer's name points around in circles"))
}

fn ragged() -> Error {
    Error::new("an answer ran past its own length")
}
