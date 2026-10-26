use crate::error::Error;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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

    pub fn host(address: IpAddr) -> Self {
        let prefix = match address {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Self {
            base: address,
            prefix,
        }
    }

    pub fn masked(self) -> Self {
        let base = match self.base {
            IpAddr::V4(base) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::from((u32::from(base) & mask).to_be_bytes())
            }
            IpAddr::V6(base) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                IpAddr::from((u128::from(base) & mask).to_be_bytes())
            }
        };
        Self { base, ..self }
    }

    pub fn base(self) -> IpAddr {
        self.base
    }

    pub fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn read(told: &str) -> Result<Self, Error> {
        let (base, prefix) = told
            .split_once('/')
            .ok_or_else(|| Error::new(format!("{told} carries no prefix length")))?;
        let base = base
            .parse()
            .map_err(|_| Error::new(format!("{base} is not an address")))?;
        let prefix = prefix
            .parse()
            .map_err(|_| Error::new(format!("{prefix} is not a prefix length")))?;
        Self::new(base, prefix)
    }

    pub fn overlaps(self, other: Self) -> bool {
        match self.prefix <= other.prefix {
            true => self.holds(other.base),
            false => other.holds(self.base),
        }
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
