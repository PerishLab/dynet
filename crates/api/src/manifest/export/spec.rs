use super::token::decode;
use dynet_core::Error;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt;
use zeroize::Zeroizing;

pub const HEALTH: &str = "http://la.vonet.online/generate_204";
pub const CADENCE: u64 = 1800;
pub const EPOCH: u64 = 86400;
pub const GRACE: u64 = 1;
const MAXGRACE: u64 = 7;
const MINSEED: usize = 16;

#[derive(Clone, Eq, PartialEq)]
pub struct Recipient {
    name: String,
    seed: Zeroizing<Vec<u8>>,
}

#[derive(Clone, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Timing {
    #[serde(default = "health", rename = "health_check_url")]
    health: String,
    #[serde(default = "cadence", rename = "content_interval")]
    cadence: u64,
    #[serde(default = "epoch", rename = "path_interval")]
    epoch: u64,
    #[serde(default = "grace", rename = "grace_slots")]
    grace: u64,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Spec {
    timing: Timing,
    recipients: Vec<Recipient>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wire {
    #[serde(flatten)]
    timing: Timing,
    recipients: Vec<Member>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    name: String,
    seed: String,
}

fn health() -> String {
    HEALTH.to_owned()
}

fn cadence() -> u64 {
    CADENCE
}

fn epoch() -> u64 {
    EPOCH
}

fn grace() -> u64 {
    GRACE
}

impl Recipient {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn seed(&self) -> &[u8] {
        &self.seed
    }
}

impl Spec {
    pub(crate) fn parse(wire: Wire) -> Result<Self, Error> {
        let timing = wire.timing;
        if timing.cadence == 0 {
            return Err(Error::new(
                "projection content_interval must be a positive number of seconds",
            ));
        }
        if timing.epoch == 0 {
            return Err(Error::new(
                "projection path_interval must be a positive number of seconds",
            ));
        }
        if timing.grace > MAXGRACE {
            return Err(Error::new(format!(
                "projection grace_slots must not exceed {MAXGRACE}"
            )));
        }
        if !url(&timing.health) {
            return Err(Error::new(
                "projection health_check_url must be an http:// or https:// URL without whitespace",
            ));
        }
        if wire.recipients.is_empty() {
            return Err(Error::new("projection must declare at least one recipient"));
        }
        let recipients = collect(wire.recipients)?;
        Ok(Self { timing, recipients })
    }

    pub fn health(&self) -> &str {
        &self.timing.health
    }

    pub fn cadence(&self) -> u64 {
        self.timing.cadence
    }

    pub fn epoch(&self) -> u64 {
        self.timing.epoch
    }

    pub fn grace(&self) -> u64 {
        self.timing.grace
    }

    pub fn recipients(&self) -> &[Recipient] {
        &self.recipients
    }

    pub fn recipient(&self, name: &str) -> Option<&Recipient> {
        self.recipients.iter().find(|item| item.name == name)
    }
}

fn collect(raw: Vec<Member>) -> Result<Vec<Recipient>, Error> {
    let mut seen = BTreeSet::new();
    let mut list = Vec::with_capacity(raw.len());
    for item in raw {
        if !label(&item.name) {
            return Err(Error::new(
                "projection recipient name must be 1..=64 ASCII letters, digits, '.', '_' or '-'",
            ));
        }
        if !seen.insert(item.name.clone()) {
            return Err(Error::new(format!(
                "projection recipient {} is declared twice",
                item.name
            )));
        }
        let bytes = decode(&item.seed)?;
        if bytes.len() < MINSEED {
            return Err(Error::new(format!(
                "projection recipient {} seed must be at least {MINSEED} bytes",
                item.name
            )));
        }
        list.push(Recipient {
            name: item.name,
            seed: Zeroizing::new(bytes),
        });
    }
    Ok(list)
}

fn label(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn url(value: &str) -> bool {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let Some(rest) = rest else {
        return false;
    };
    !rest.is_empty()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

impl fmt::Debug for Recipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Recipient([redacted])")
    }
}

impl fmt::Debug for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spec")
            .field("cadence", &self.timing.cadence)
            .field("epoch", &self.timing.epoch)
            .field("grace", &self.timing.grace)
            .field("recipients", &self.recipients.len())
            .finish()
    }
}
