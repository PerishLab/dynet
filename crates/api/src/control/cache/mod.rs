mod checkpoint;
use super::{Candidate, Decoder, Incoming, custody, refused};
pub use checkpoint::Checkpoint;
use dynet_core::{Error, Ledger};
use dynet_host::Private;
use secrets::{Assignment, Kind, Receipt};
use serde_json::value::RawValue;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const LIMIT: usize = 16 * 1024 * 1024;

pub struct Cache {
    root: PathBuf,
    private: Private,
    checkpoint: Mutex<Checkpoint>,
    _lock: File,
}

impl Cache {
    pub fn open(root: &Path, trust: String, private: Private) -> Result<Self, Error> {
        private.folder(root).map_err(custody)?;
        let lock = private.lock(&root.join("lock")).map_err(custody)?;
        let checkpoint = match private.read(&root.join("checkpoint"), LIMIT) {
            Ok(bytes) => {
                if private
                    .read(&root.join("initialized"), 64)
                    .map_err(custody)?
                    != b"dynet-control-v1"
                {
                    return Err(refused());
                }
                serde_json::from_slice::<Checkpoint>(&bytes).map_err(|_| refused())?
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                empty(root)?;
                Checkpoint {
                    version: 1,
                    trust: trust.clone(),
                    received: None,
                    validated: None,
                    applied: None,
                    retained: None,
                    barred: None,
                    ledger: RawValue::from_string("[]".to_owned()).map_err(|_| refused())?,
                }
            }
            Err(error) => return Err(custody(error)),
        };
        checkpoint.check(&trust)?;
        Ok(Self {
            root: root.to_owned(),
            private,
            checkpoint: Mutex::new(checkpoint),
            _lock: lock,
        })
    }

    pub fn checkpoint(&self) -> Result<Checkpoint, Error> {
        self.lock().map(|checkpoint| checkpoint.clone())
    }

    pub fn receive(&self, incoming: &Incoming) -> Result<(), Error> {
        let mut checkpoint = self.lock()?;
        if let Some(earlier) = &checkpoint.received {
            incoming
                .assignment
                .receipt
                .follows(&earlier.receipt)
                .map_err(|_| refused())?;
        }
        let path = self.blob(&incoming.assignment.receipt);
        match self.private.create(&path, &incoming.bytes) {
            Ok(()) => (),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if self.private.read(&path, secrets::LIMIT).map_err(custody)? != incoming.bytes {
                    return Err(refused());
                }
            }
            Err(error) => return Err(custody(error)),
        }
        let mut next = checkpoint.clone();
        next.received = Some(incoming.assignment.clone());
        if incoming.assignment.kind == Kind::Revocation {
            next.barred = Some(incoming.assignment.clone());
        }
        self.commit(&mut checkpoint, next)
    }

    pub fn validate(&self, candidate: &Candidate) -> Result<(), Error> {
        let mut checkpoint = self.lock()?;
        if checkpoint.received.as_ref() != Some(&candidate.assignment) {
            return Err(refused());
        }
        let mut next = checkpoint.clone();
        next.validated = Some(candidate.assignment.receipt.clone());
        self.commit(&mut checkpoint, next)
    }

    pub fn apply(&self, assignment: &Assignment, ledger: &Ledger) -> Result<(), Error> {
        let mut checkpoint = self.lock()?;
        let revoked = checkpoint.barred.as_ref() == Some(assignment);
        if !revoked
            && (checkpoint.received.as_ref() != Some(assignment)
                || checkpoint.validated.as_ref() != Some(&assignment.receipt))
        {
            return Err(refused());
        }
        let mut next = checkpoint.clone();
        next.applied = Some(assignment.clone());
        if next
            .validated
            .as_ref()
            .is_none_or(|receipt| receipt.generation < assignment.receipt.generation)
        {
            next.validated = Some(assignment.receipt.clone());
        }
        if assignment.kind == Kind::Configuration {
            next.retained = Some(assignment.clone());
            next.barred = None;
        }
        next.ledger = RawValue::from_string(ledger.write()).map_err(|_| refused())?;
        self.commit(&mut checkpoint, next)
    }

    pub fn save(&self, ledger: &Ledger) -> Result<(), Error> {
        let mut checkpoint = self.lock()?;
        let mut next = checkpoint.clone();
        next.ledger = RawValue::from_string(ledger.write()).map_err(|_| refused())?;
        self.commit(&mut checkpoint, next)
    }

    pub fn restore(&self, decoder: &Decoder) -> Result<Option<Candidate>, Error> {
        let checkpoint = self.checkpoint()?;
        let assignment = checkpoint.barred.as_ref().or(checkpoint.applied.as_ref());
        assignment
            .map(|assignment| self.decode(decoder, assignment))
            .transpose()
    }

    pub fn waiting(&self, decoder: &Decoder) -> Result<Option<Candidate>, Error> {
        let checkpoint = self.checkpoint()?;
        let Some(received) = checkpoint.received.as_ref() else {
            return Ok(None);
        };
        if checkpoint.applied.as_ref() == Some(received)
            || checkpoint.validated.as_ref() != Some(&received.receipt)
        {
            return Ok(None);
        }
        self.decode(decoder, received).map(Some)
    }

    pub fn protection(&self, decoder: &Decoder) -> Result<Option<Candidate>, Error> {
        let checkpoint = self.checkpoint()?;
        checkpoint
            .retained
            .as_ref()
            .map(|assignment| self.decode(decoder, assignment))
            .transpose()
    }

    fn decode(&self, decoder: &Decoder, assignment: &Assignment) -> Result<Candidate, Error> {
        let bytes = self
            .private
            .read(&self.blob(&assignment.receipt), secrets::LIMIT)
            .map_err(custody)?;
        let incoming = decoder.receive(bytes, None)?;
        if &incoming.assignment != assignment {
            return Err(refused());
        }
        decoder.open(incoming)
    }

    fn blob(&self, receipt: &Receipt) -> PathBuf {
        self.root.join(format!("{}.envelope", receipt.digest))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Checkpoint>, Error> {
        self.checkpoint.lock().map_err(|_| refused())
    }

    fn collect(&self, checkpoint: &Checkpoint) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        let keep: Vec<_> = [
            &checkpoint.received,
            &checkpoint.applied,
            &checkpoint.retained,
            &checkpoint.barred,
        ]
        .into_iter()
        .flatten()
        .map(|assignment| self.blob(&assignment.receipt))
        .collect();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let owned = path
                .extension()
                .is_some_and(|extension| extension == "envelope")
                && stem.len() == 64
                && stem.bytes().all(|byte| byte.is_ascii_hexdigit());
            if owned && !keep.contains(&path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn commit(&self, checkpoint: &mut Checkpoint, next: Checkpoint) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&next).map_err(|_| refused())?;
        if bytes.len() > LIMIT {
            return Err(refused());
        }
        match self
            .private
            .create(&self.root.join("initialized"), b"dynet-control-v1")
        {
            Ok(()) => (),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if self
                    .private
                    .read(&self.root.join("initialized"), 64)
                    .map_err(custody)?
                    != b"dynet-control-v1"
                {
                    return Err(refused());
                }
            }
            Err(error) => return Err(custody(error)),
        }
        self.private
            .replace(&self.root.join("checkpoint"), &bytes)
            .map_err(custody)?;
        *checkpoint = next;
        self.collect(checkpoint);
        Ok(())
    }
}

fn empty(root: &Path) -> Result<(), Error> {
    for entry in std::fs::read_dir(root).map_err(custody)? {
        if entry.map_err(custody)?.file_name() != "lock" {
            return Err(refused());
        }
    }
    Ok(())
}
