use crate::error::Error;
use std::net::Ipv4Addr;

const NARROW: u8 = 30;
const BROAD: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    base: Ipv4Addr,
    prefix: u8,
}

impl Span {
    pub fn new(value: &str) -> Result<Self, Error> {
        let (told, width) = value
            .split_once('/')
            .ok_or_else(|| Error::new(format!("{value} carries no prefix length")))?;
        let base: Ipv4Addr = told
            .parse()
            .map_err(|_| Error::new(format!("{told} is not an address")))?;
        let prefix: u8 = width
            .parse()
            .map_err(|_| Error::new(format!("{width} is not a prefix length")))?;
        if !(BROAD..=NARROW).contains(&prefix) {
            return Err(Error::new(format!(
                "a span must lie between {BROAD} and {NARROW} bits so it can seat a device and a spare, not {prefix}"
            )));
        }
        if u32::from(base) & !mask(prefix) != 0 {
            return Err(Error::new(format!(
                "{base} carries bits below its own {prefix} bit prefix and is not the base of a span"
            )));
        }
        Ok(Self { base, prefix })
    }

    pub fn get(self) -> String {
        format!("{}/{}", self.base, self.prefix)
    }

    pub fn seat(self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.base) + 1)
    }

    pub fn spare(self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.base) + self.half())
    }

    pub fn worn(self) -> String {
        format!("{}/{}", self.seat(), self.prefix)
    }

    fn half(self) -> u32 {
        1 << (31 - self.prefix)
    }
}

fn mask(prefix: u8) -> u32 {
    match prefix {
        0 => 0,
        _ => u32::MAX << (32 - prefix),
    }
}
