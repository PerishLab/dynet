use super::{Query, skip};
use dynet_core::{Domain, Error};
use std::net::Ipv4Addr;

pub(super) enum Data {
    Alias(Domain),
    Tree(Domain),
    Address(Ipv4Addr),
    Other,
}

pub(super) struct Record {
    pub owner: Domain,
    pub kind: u16,
    pub life: u32,
    pub data: Data,
}

pub(super) fn records(body: &[u8], query: &Query) -> Result<Vec<Record>, Error> {
    let mut cursor = query.span;
    let counts = body.get(6..12).ok_or_else(invalid)?;
    let mut records = Vec::new();
    for (section, bytes) in counts.chunks_exact(2).enumerate() {
        for _ in 0..u16::from_be_bytes([bytes[0], bytes[1]]) {
            let owner = cursor;
            cursor = skip(body, cursor)?;
            let fixed = body.get(cursor..cursor + 10).ok_or_else(invalid)?;
            let kind = u16::from_be_bytes([fixed[0], fixed[1]]);
            let class = u16::from_be_bytes([fixed[2], fixed[3]]);
            let life = u32::from_be_bytes(fixed[4..8].try_into().map_err(|_| invalid())?);
            let size = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
            cursor += 10;
            let raw = body.get(cursor..cursor + size).ok_or_else(invalid)?;
            if section == 0 && class == 1 {
                records.push(Record {
                    owner: name(body, owner)?,
                    kind,
                    data: data(body, cursor, raw, kind)?,
                    life,
                });
            }
            cursor += size;
        }
    }
    if cursor != body.len() {
        return Err(invalid());
    }
    Ok(records)
}

fn data(body: &[u8], start: usize, raw: &[u8], kind: u16) -> Result<Data, Error> {
    match kind {
        1 => Ok(Data::Address(Ipv4Addr::from(
            <[u8; 4]>::try_from(raw).map_err(|_| invalid())?,
        ))),
        5 | 39 => {
            if skip(body, start)? != start + raw.len() {
                return Err(invalid());
            }
            let target = name(body, start)?;
            Ok(if kind == 5 {
                Data::Alias(target)
            } else {
                Data::Tree(target)
            })
        }
        64 | 65 if raw.starts_with(&[0, 0]) => {
            if raw.len() < 3 || skip(body, start + 2)? > start + raw.len() {
                return Err(invalid());
            }
            Ok(Data::Alias(name(body, start + 2)?))
        }
        _ => Ok(Data::Other),
    }
}

fn name(body: &[u8], mut cursor: usize) -> Result<Domain, Error> {
    let mut labels = Vec::new();
    let mut size = 0;
    let mut pointers = 0;
    loop {
        let byte = *body.get(cursor).ok_or_else(invalid)?;
        if byte & 0xc0 == 0xc0 {
            let low = *body.get(cursor + 1).ok_or_else(invalid)?;
            let target = usize::from(u16::from_be_bytes([byte & 63, low]));
            if target >= cursor || pointers >= 64 {
                return Err(invalid());
            }
            pointers += 1;
            cursor = target;
            continue;
        }
        if byte == 0 {
            return Domain::new(labels.join("."));
        }
        if byte > 63 {
            return Err(invalid());
        }
        let end = cursor + 1 + usize::from(byte);
        let label = std::str::from_utf8(body.get(cursor + 1..end).ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        if label.contains('.') {
            return Err(invalid());
        }
        size += label.len() + 1;
        if size > 254 {
            return Err(invalid());
        }
        labels.push(label);
        cursor = end;
    }
}

pub(super) fn invalid() -> Error {
    Error::new("invalid DNS record or alias chain")
}
