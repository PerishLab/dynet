use super::super::Checkpoint;
use crate::host::State;
use dynet_core::{Error, Instance};
use dynet_host::Private;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const VERSION: u8 = 1;
const LIMIT: usize = 64 * 1024;
const CHECKPOINT: usize = 16 * 1024 * 1024;
const BUSY: Duration = Duration::from_secs(20);
const RETRY: Duration = Duration::from_millis(25);

pub(super) const CANDIDATE: &str = "candidate.envelope";
pub(super) const ROLLBACK: &str = "rollback.envelope";
const STATE: &str = "state.json";
const LOCK: &str = "lock";
const READY: &str = "ready";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Armed,
    Applying,
    Active,
    Reverting,
    Confirmed,
    Reverted,
    Failed,
    Unknown,
}

impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Reverted)
    }

    pub fn retained(self) -> bool {
        matches!(self, Self::Failed | Self::Unknown)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    pub generation: u64,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u8,
    pub id: String,
    pub instance: String,
    pub phase: Phase,
    pub bootstrap: PathBuf,
    pub target: PathBuf,
    pub window: u64,
    pub settle: u64,
    pub boot: Option<String>,
    pub deadline: Option<u64>,
    pub watcher: Option<u32>,
    pub prior: Option<Sealed>,
    pub candidate: Sealed,
    pub rollback: Sealed,
    pub reason: Option<String>,
}

pub(super) struct Vault {
    parent: PathBuf,
    root: PathBuf,
    control: PathBuf,
    private: Private,
}

impl Vault {
    pub(super) fn new(instance: &Instance, state: &State, private: Private) -> Self {
        let parent = state.root().to_owned();
        let root = parent.join(format!("{}.cutover", instance.get()));
        let control = parent.join(format!("{}.control", instance.get()));
        Self {
            parent,
            root,
            control,
            private,
        }
    }

    pub(super) fn private(&self) -> Private {
        self.private
    }

    pub(super) fn control(&self) -> &Path {
        &self.control
    }

    pub(super) fn lock(&self) -> Result<File, Error> {
        self.private.folder(&self.parent).map_err(custody)?;
        self.private.folder(&self.root).map_err(custody)?;
        let path = self.root.join(LOCK);
        let until = Instant::now() + BUSY;
        loop {
            match self.private.lock(&path) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                    return Err(custody(error));
                }
                Err(_) if Instant::now() < until => thread::sleep(RETRY),
                Err(_) => return Err(Error::new("cutover transaction lock timed out")),
            }
        }
    }

    pub(super) fn load(&self) -> Result<Option<Record>, Error> {
        match self.private.read(&self.root.join(STATE), LIMIT) {
            Ok(bytes) => {
                let record: Record = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::new("cutover transaction state is corrupt"))?;
                if record.version != VERSION || !valid(&record.id) {
                    return Err(Error::new("cutover transaction state is invalid"));
                }
                Ok(Some(record))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(custody(error)),
        }
    }

    pub(super) fn save(&self, record: &Record) -> Result<(), Error> {
        let bytes = serde_json::to_vec(record)
            .map_err(|_| Error::new("cannot encode cutover transaction state"))?;
        if bytes.len() > LIMIT {
            return Err(Error::new("cutover transaction state exceeds size bound"));
        }
        self.private
            .replace(&self.root.join(STATE), &bytes)
            .map_err(custody)
    }

    pub(super) fn stage(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        self.private
            .replace(&self.root.join(name), bytes)
            .map_err(custody)
    }

    pub(super) fn staged(&self, name: &str, expected: &str) -> Result<Vec<u8>, Error> {
        let bytes = self
            .private
            .read(&self.root.join(name), secrets::LIMIT)
            .map_err(custody)?;
        if digest(&bytes) != expected {
            return Err(Error::new(format!(
                "staged {name} digest does not match cutover transaction"
            )));
        }
        Ok(bytes)
    }

    pub(super) fn mark(&self, id: &str, pid: u32) -> Result<(), Error> {
        let content = format!("{id} {pid}\n");
        self.private
            .replace(&self.root.join(READY), content.as_bytes())
            .map_err(custody)
    }

    pub(super) fn ready(&self, id: &str) -> Result<Option<u32>, Error> {
        let bytes = match self.private.read(&self.root.join(READY), 256) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(custody(error)),
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return Ok(None);
        };
        let mut parts = text.split_whitespace();
        let Some(found) = parts.next() else {
            return Ok(None);
        };
        let Some(pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            return Ok(None);
        };
        if found == id && parts.next().is_none() {
            Ok(Some(pid))
        } else {
            Ok(None)
        }
    }

    pub(super) fn reset(&self) {
        let _ = std::fs::remove_file(self.root.join(READY));
    }

    pub(super) fn clean(&self) {
        for name in [CANDIDATE, ROLLBACK, READY] {
            let _ = std::fs::remove_file(self.root.join(name));
        }
    }

    pub(super) fn purge(&self) -> Result<(), Error> {
        self.clean();
        match std::fs::remove_file(self.root.join(STATE)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(custody(error)),
        }
    }

    pub(super) fn checkpoint(&self) -> Result<Option<Checkpoint>, Error> {
        match self
            .private
            .read(&self.control.join("checkpoint"), CHECKPOINT)
        {
            Ok(bytes) => serde_json::from_slice::<Checkpoint>(&bytes)
                .map(Some)
                .map_err(|_| Error::new("control checkpoint is corrupt")),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(custody(error)),
        }
    }

    pub(super) fn clearable(&self) -> Result<(), Error> {
        let Some(existing) = self.load()? else {
            return Ok(());
        };
        if existing.phase.terminal() {
            return Ok(());
        }
        if existing.phase.retained() {
            return Err(Error::new(format!(
                "cutover {} retains {:?} state; inspect and clear before arming",
                existing.id, existing.phase
            )));
        }
        Err(Error::new(format!(
            "cutover {} is already active in {:?} state",
            existing.id, existing.phase
        )))
    }

    pub(super) fn require(&self, id: &str) -> Result<Record, Error> {
        if !valid(id) {
            return Err(Error::new("cutover transaction id is invalid"));
        }
        let record = self
            .load()?
            .ok_or_else(|| Error::new("no cutover transaction is recorded"))?;
        if record.id != id {
            return Err(Error::new(
                "cutover transaction id does not match recorded transaction",
            ));
        }
        Ok(record)
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn token() -> String {
    let mut raw = [0u8; 16];
    OsRng.fill_bytes(&mut raw);
    let mut text = String::with_capacity(32);
    for byte in raw {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

pub(super) fn valid(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn custody(_: std::io::Error) -> Error {
    Error::new("private cutover state refused")
}
