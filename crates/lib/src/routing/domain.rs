use crate::error::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Domain(String);

impl<'de> Deserialize<'de> for Domain {
    fn deserialize<D: Deserializer<'de>>(told: D) -> Result<Self, D::Error> {
        let value = String::deserialize(told)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Domain {
    fn serialize<S: Serializer>(&self, told: S) -> Result<S::Ok, S::Error> {
        told.serialize_str(&self.0)
    }
}

impl Domain {
    pub fn new(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref().trim_end_matches('.').to_ascii_lowercase();
        if value.is_empty() || value.len() > 253 {
            return Err(Error::new("a domain must carry between one and 253 bytes"));
        }
        for label in value.split('.') {
            if label.is_empty() || label.len() > 63 {
                return Err(Error::new(format!(
                    "domain {value} carries an unusable label"
                )));
            }
            if !label.bytes().all(usable) {
                return Err(Error::new(format!(
                    "domain {value} carries an unusable byte"
                )));
            }
        }
        Ok(Self(value))
    }

    pub fn get(&self) -> &str {
        &self.0
    }

    pub fn within(&self, parent: &Self) -> bool {
        if self.0 == parent.0 {
            return true;
        }
        self.0
            .strip_suffix(&parent.0)
            .and_then(|head| head.strip_suffix('.'))
            .is_some_and(|head| !head.is_empty())
    }
}

fn usable(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}
