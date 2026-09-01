use crate::error::Error;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Range {
    base: IpAddr,
    prefix: u8,
}

impl Range {
    pub fn new(base: IpAddr, prefix: u8) -> Result<Self, Error> {
        let width = match base {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > width {
            return Err(Error::new(format!(
                "prefix {prefix} exceeds the {width} bits of {base}"
            )));
        }
        Ok(Self { base, prefix })
    }

    pub fn base(self) -> IpAddr {
        self.base
    }

    pub fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn holds(self, address: IpAddr) -> bool {
        match (self.base, address) {
            (IpAddr::V4(base), IpAddr::V4(address)) => {
                shared(&base.octets(), &address.octets(), self.prefix)
            }
            (IpAddr::V6(base), IpAddr::V6(address)) => {
                shared(&base.octets(), &address.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

fn shared(base: &[u8], address: &[u8], prefix: u8) -> bool {
    let whole = usize::from(prefix / 8);
    if base[..whole] != address[..whole] {
        return false;
    }
    let remainder = prefix % 8;
    if remainder == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - remainder);
    base[whole] & mask == address[whole] & mask
}
