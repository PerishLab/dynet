use super::records::{Data, Record, invalid, records};
use super::{Answer, Packet, Query};
use dynet_core::{Domain, Error};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub links: BTreeMap<Domain, u64>,
    pub addresses: Vec<Answer>,
}

impl Chain {
    pub fn extend(
        &mut self,
        requested: &[u8],
        reply: &[u8],
        now: u64,
    ) -> Result<Option<Domain>, Error> {
        let question = Packet::new(requested).asked()?;
        let query = Packet::new(reply).asked()?;
        let header = requested[..2] == reply[..2] && reply[2] & 0xf8 == 0x80;
        let same = question.name.eq_ignore_ascii_case(&query.name) && query.kind == question.kind;
        if !header
            || !same
            || reply[query.span - 2..query.span] != requested[question.span - 2..question.span]
        {
            return Err(invalid());
        }
        if reply[2] & 2 != 0 {
            return Ok(None);
        }
        let records = records(reply, &query)?;
        let first = Domain::new(&query.name)?;
        let mut current = first.clone();
        let mut until = self.links.get(&current).copied().unwrap_or(u64::MAX);
        loop {
            let edge = next(&records, &current)?;
            let before = self.addresses.len();
            for record in records.iter().filter(|item| item.owner == current) {
                if let Data::Address(address) = record.data {
                    let life = u64::from(record.life).min(until.saturating_sub(now)) as u32;
                    self.addresses.push(Answer { address, life });
                }
            }
            let present = before != self.addresses.len();
            if present && edge.is_some() {
                return Err(invalid());
            }
            if let Some((target, life)) = edge {
                until = until.min(now + u64::from(life));
                if target == first
                    || self.links.len() >= 16
                    || self.links.insert(target.clone(), until).is_some()
                {
                    return Err(invalid());
                }
                current = target;
                continue;
            }
            return Ok((current != first && !present && reply[3] & 15 == 0).then_some(current));
        }
    }

    pub fn metadata(body: &[u8], query: &Query) -> Result<Option<Vec<u8>>, Error> {
        let root = Domain::new(&query.name)?;
        let rows = records(body, query)?;
        if rows
            .iter()
            .any(|row| row.kind != query.kind || row.owner != root)
        {
            return Ok(None);
        }
        let mut end = query.span;
        for _ in 0..u16::from_be_bytes([body[6], body[7]]) {
            let fixed = super::skip(body, end)?;
            let size = u16::from_be_bytes([body[fixed + 8], body[fixed + 9]]);
            end = fixed + 10 + usize::from(size);
        }
        let mut answer = body[..end].to_vec();
        answer[8..12].fill(0);
        answer[3] &= !0x20;
        Ok(Some(answer))
    }

    pub fn project(&self, request: &[u8], query: &Query) -> Result<Vec<u8>, Error> {
        let mut reply = Packet::new(request).barren(query);
        let count = u16::try_from(self.addresses.len()).map_err(|_| invalid())?;
        reply[6..8].copy_from_slice(&count.to_be_bytes());
        for item in &self.addresses {
            reply.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
            reply.extend_from_slice(&item.life.to_be_bytes());
            reply.extend_from_slice(&[0, 4]);
            reply.extend_from_slice(&item.address.octets());
        }
        Ok(reply)
    }
}

fn next(records: &[Record], name: &Domain) -> Result<Option<(Domain, u32)>, Error> {
    let mut edge: Option<(Domain, u32)> = None;
    for item in records {
        let target = match &item.data {
            Data::Alias(target) if item.owner == *name => target.clone(),
            Data::Tree(target) if name != &item.owner && name.within(&item.owner) => {
                let head = name
                    .get()
                    .strip_suffix(item.owner.get())
                    .ok_or_else(invalid)?;
                Domain::new(format!("{head}{}", target.get()))?
            }
            _ => continue,
        };
        if let Some((held, life)) = &mut edge {
            if *held != target {
                return Err(invalid());
            }
            *life = (*life).min(item.life);
        } else {
            edge = Some((target, item.life));
        }
    }
    Ok(edge)
}

pub(crate) fn bindings(body: &[u8], query: &Query) -> Result<Vec<(std::net::IpAddr, u32)>, Error> {
    let rows = records(body, query)?;
    let mut name = Domain::new(&query.name)?;
    let mut life = u32::MAX;
    for _ in 0..=16 {
        let edge = next(&rows, &name)?;
        let mut found = Vec::new();
        for row in rows
            .iter()
            .filter(|row| row.owner == name && row.kind == query.kind)
        {
            let address = match row.data {
                Data::Address(ip) => std::net::IpAddr::V4(ip),
                Data::Quad(ip) => std::net::IpAddr::V6(ip),
                _ => continue,
            };
            found.push((address, life.min(row.life)));
        }
        let Some((target, ttl)) = edge else {
            return Ok(found);
        };
        if !found.is_empty() {
            return Err(invalid());
        }
        name = target;
        life = life.min(ttl);
    }
    Err(invalid())
}
