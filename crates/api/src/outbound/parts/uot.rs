use dynet_core::Error;
use std::collections::VecDeque;
use std::net::Ipv4Addr;

pub(super) const HOST: &str = "sp.v2.udp-over-tcp.arpa";
const LIMIT: usize = 65_507;

pub(super) fn request(seat: (&str, u16)) -> Result<[u8; 8], Error> {
    let address = seat
        .0
        .parse::<Ipv4Addr>()
        .map_err(|_| Error::new("UDP over TCP requires an IPv4 destination"))?;
    let [a, b, c, d] = address.octets();
    let [high, low] = seat.1.to_be_bytes();
    Ok([1, 1, a, b, c, d, high, low])
}

pub(super) fn frame(body: &[u8]) -> Result<Vec<u8>, Error> {
    if body.len() > LIMIT {
        return Err(Error::new("UDP datagram exceeds the IPv4 payload limit"));
    }
    let mut framed = Vec::with_capacity(body.len() + 2);
    framed.extend_from_slice(&(body.len() as u16).to_be_bytes());
    framed.extend_from_slice(body);
    Ok(framed)
}

#[derive(Default)]
pub struct Frames {
    pending: VecDeque<u8>,
}

impl Frames {
    pub(super) fn receive(
        &mut self,
        mut read: impl FnMut() -> Result<Option<Vec<u8>>, Error>,
    ) -> Result<Option<Vec<u8>>, Error> {
        loop {
            if let Some(body) = self.take()? {
                return Ok(Some(body));
            }
            match read()? {
                Some(chunk) => self.pending.extend(chunk),
                None if self.pending.is_empty() => return Ok(None),
                None => return Err(Error::new("UDP relay closed mid-frame")),
            }
        }
    }

    fn take(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.pending.len() < 2 {
            return Ok(None);
        }
        let size = usize::from(u16::from_be_bytes([self.pending[0], self.pending[1]]));
        if size > LIMIT {
            return Err(Error::new("UDP relay frame exceeds the IPv4 payload limit"));
        }
        if self.pending.len() < size + 2 {
            return Ok(None);
        }
        Ok(Some(self.pending.drain(..size + 2).skip(2).collect()))
    }
}

#[cfg(test)]
mod tests;
