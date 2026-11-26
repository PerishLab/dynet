use super::state::{Phase, ROLLBACK, Record, Sealed, Vault, custody};
use dynet_core::Error;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_millis(100);
const WAIT: Duration = Duration::from_secs(5);
const CHECK: Duration = Duration::from_millis(25);

pub(super) fn clock() -> Result<(String, u64), Error> {
    dynet_host::linux::boot()
        .map_err(|error| Error::new(format!("cannot read kernel boot clock: {error}")))
}

pub(super) fn expired(record: &Record, boot: &str, now: u64) -> bool {
    record.boot.as_deref() != Some(boot) || record.deadline.is_none_or(|deadline| now >= deadline)
}

impl Vault {
    pub(super) fn awaiter(&self, id: &str) -> Result<Option<u32>, Error> {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(pid) = self.ready(id)?
                && dynet_host::linux::alive(pid)
            {
                return Ok(Some(pid));
            }
            if Instant::now() >= until {
                return Ok(None);
            }
            thread::sleep(CHECK);
        }
    }

    pub(super) fn install(&self, target: &Path, bytes: &[u8]) -> Result<(), Error> {
        self.private().replace(target, bytes).map_err(custody)
    }

    pub(super) fn applied(&self, expected: &Sealed) -> Result<bool, Error> {
        let Some(checkpoint) = self.checkpoint()? else {
            return Ok(true);
        };
        Ok(checkpoint.applied.as_ref().is_some_and(|assignment| {
            assignment.receipt.generation == expected.generation
                && assignment.receipt.digest == expected.digest
        }))
    }

    pub(super) fn converge(&self, expected: &Sealed, settle: u64) -> Result<bool, Error> {
        if self.checkpoint()?.is_none() {
            return Ok(true);
        }
        let until = Instant::now() + Duration::from_secs(settle);
        loop {
            let Some(checkpoint) = self.checkpoint()? else {
                return Ok(false);
            };
            if checkpoint.applied.as_ref().is_some_and(|assignment| {
                assignment.receipt.generation == expected.generation
                    && assignment.receipt.digest == expected.digest
            }) {
                return Ok(true);
            }
            if checkpoint.received.as_ref().is_some_and(|assignment| {
                assignment.receipt.generation > expected.generation
                    || (assignment.receipt.generation == expected.generation
                        && assignment.receipt.digest != expected.digest)
            }) {
                return Ok(false);
            }
            if Instant::now() >= until {
                return Ok(false);
            }
            thread::sleep(STEP);
        }
    }

    pub(super) fn revert(&self, record: &mut Record, settle: u64) -> Result<(), Error> {
        let bytes = match self.staged(ROLLBACK, &record.rollback.digest) {
            Ok(bytes) => bytes,
            Err(error) => {
                record.phase = Phase::Failed;
                record.reason = Some(error.to_string());
                let _ = self.save(record);
                return Err(error);
            }
        };
        record.phase = Phase::Reverting;
        record.reason = None;
        self.save(record)?;
        if let Err(error) = self.install(&record.target, &bytes) {
            record.phase = Phase::Failed;
            record.reason = Some(format!("rollback install failed: {error}"));
            let _ = self.save(record);
            return Err(error);
        }
        match self.converge(&record.rollback, settle) {
            Ok(true) => {
                record.phase = Phase::Reverted;
                record.reason = None;
                self.save(record)?;
                self.clean();
                Ok(())
            }
            Ok(false) => {
                let message = format!(
                    "rollback generation {} installed but worker hot-load did not confirm within {settle}s",
                    record.rollback.generation
                );
                record.phase = Phase::Unknown;
                record.reason = Some(message.clone());
                let _ = self.save(record);
                Err(Error::new(message))
            }
            Err(error) => {
                record.phase = Phase::Unknown;
                record.reason = Some(format!("rollback hot-load check failed: {error}"));
                let _ = self.save(record);
                Err(error)
            }
        }
    }
}
