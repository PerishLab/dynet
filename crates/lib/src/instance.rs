use crate::error::Error;

const PREFIX: &str = "dynet";
const BASE: u32 = 17000;
const SPAN: u32 = 1000;
const STAMP: u32 = 0x6479_0000;
const SEED: u64 = 0xcbf2_9ce4_8422_2325;
const ODD: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instance(String);

impl Instance {
    pub fn new(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref().trim().to_string();
        if !value.starts_with(PREFIX) {
            return Err(Error::new(format!(
                "an instance name must begin with {PREFIX} so a sweep can recognise it, not {value}"
            )));
        }
        if value.len() > 15 || !value.bytes().all(usable) {
            return Err(Error::new(format!(
                "instance {value} must fit an interface name and carry only letters and digits"
            )));
        }
        Ok(Self(value))
    }

    pub fn get(&self) -> &str {
        &self.0
    }

    pub fn priority(&self) -> u32 {
        BASE + self.spread()
    }

    pub fn mark(&self) -> u32 {
        STAMP + self.spread()
    }

    fn spread(&self) -> u32 {
        u32::try_from(scramble(&self.0) % u64::from(SPAN)).unwrap_or_default()
    }
}

fn usable(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

fn scramble(value: &str) -> u64 {
    let mut state = SEED;
    for byte in value.bytes() {
        state ^= u64::from(byte);
        state = state.wrapping_mul(ODD);
    }
    state
}
