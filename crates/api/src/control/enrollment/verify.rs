use super::super::{Checkpoint, Payload};
use super::bootstrap::{Bootstrap, Source};
use super::state::{Sealed, Vault, custody, digest};
use dynet_core::Error;
use secrets::{Identity, Kind, Reader};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub(super) struct Pair {
    pub target: PathBuf,
    pub prior: Option<Sealed>,
    pub candidate: Sealed,
    pub rollback: Sealed,
    pub proposed: Vec<u8>,
    pub backup: Vec<u8>,
}

struct Opened {
    sealed: Sealed,
    payload: Payload,
}

struct Checker<'a> {
    vault: &'a Vault,
    reader: Reader,
    identity: Identity,
}

pub(super) fn inspect(
    vault: &Vault,
    bootstrap: &Path,
    candidate: &Path,
    rollback: &Path,
) -> Result<Pair, Error> {
    let config = Bootstrap::read(bootstrap, vault.private())?;
    let target = match &config.source {
        Source::File { path } => path.clone(),
        Source::Url { .. } => {
            return Err(Error::new(
                "paired-envelope cutover requires a file bootstrap source",
            ));
        }
    };
    let checker = Checker {
        vault,
        reader: config.reader()?,
        identity: config.identity(vault.private())?,
    };
    let proposed = vault
        .private()
        .read(candidate, secrets::LIMIT)
        .map_err(custody)?;
    let backup = vault
        .private()
        .read(rollback, secrets::LIMIT)
        .map_err(custody)?;
    let next = checker.open(&proposed, "candidate")?;
    let revert = checker.open(&backup, "rollback")?;
    let expected = next
        .sealed
        .generation
        .checked_add(1)
        .ok_or_else(|| Error::new("candidate generation overflows"))?;
    if revert.sealed.generation != expected {
        return Err(Error::new(format!(
            "rollback generation {} must equal candidate generation + 1 ({expected})",
            revert.sealed.generation
        )));
    }
    let checkpoint = vault.checkpoint()?;
    let live = checker.current(&target, checkpoint.as_ref())?;
    let floor = floor(checkpoint.as_ref(), live.as_ref());
    if floor > 0 && next.sealed.generation <= floor {
        return Err(Error::new(format!(
            "candidate generation {} must exceed current generation floor {floor}",
            next.sealed.generation
        )));
    }
    if let Some(active) = &live
        && !same(&revert.payload, &active.payload)
    {
        return Err(Error::new(format!(
            "rollback payload (generation {}) does not match current confirmed policy",
            revert.sealed.generation
        )));
    }
    Ok(Pair {
        target,
        prior: live.map(|active| active.sealed),
        candidate: next.sealed,
        rollback: revert.sealed,
        proposed,
        backup,
    })
}

impl Checker<'_> {
    fn open(&self, bytes: &[u8], role: &str) -> Result<Opened, Error> {
        let verified = self
            .reader
            .verify(bytes, None)
            .map_err(|_| Error::new(format!("{role} envelope authentication refused")))?;
        if verified.kind() != Kind::Configuration {
            return Err(Error::new(format!(
                "{role} envelope must be a configuration envelope"
            )));
        }
        let receipt = verified.receipt().clone();
        let opened = verified
            .open(&self.identity)
            .map_err(|_| Error::new(format!("{role} envelope decryption refused")))?;
        let payload = Payload::check(opened.content())
            .map_err(|error| Error::new(format!("{role} payload invalid: {error}")))?;
        if receipt.digest != digest(bytes) {
            return Err(Error::new(format!("{role} envelope digest mismatch")));
        }
        Ok(Opened {
            sealed: Sealed {
                generation: receipt.generation,
                digest: receipt.digest,
            },
            payload,
        })
    }

    fn current(
        &self,
        target: &Path,
        checkpoint: Option<&Checkpoint>,
    ) -> Result<Option<Opened>, Error> {
        if let Some(applied) = checkpoint.and_then(|item| item.applied.as_ref())
            && applied.kind == Kind::Configuration
        {
            let blob = self
                .vault
                .control()
                .join(format!("{}.envelope", applied.receipt.digest));
            let bytes = self
                .vault
                .private()
                .read(&blob, secrets::LIMIT)
                .map_err(custody)?;
            return self.open(&bytes, "confirmed").map(Some);
        }
        match self.vault.private().read(target, secrets::LIMIT) {
            Ok(bytes) => self.open(&bytes, "current").map(Some),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(custody(error)),
        }
    }
}

fn floor(checkpoint: Option<&Checkpoint>, live: Option<&Opened>) -> u64 {
    let mut ceiling = live.map_or(0, |active| active.sealed.generation);
    if let Some(checkpoint) = checkpoint {
        for assignment in [&checkpoint.received, &checkpoint.applied]
            .into_iter()
            .flatten()
        {
            ceiling = ceiling.max(assignment.receipt.generation);
        }
    }
    ceiling
}

fn same(left: &Payload, right: &Payload) -> bool {
    left.upstream == right.upstream
        && left.retain == right.retain
        && left.loaded.manifest() == right.loaded.manifest()
}
