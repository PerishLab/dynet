pub use super::state::{Phase, Record, Sealed};

use super::converge::{clock, expired};
use super::state::{CANDIDATE, ROLLBACK, Vault, token};
use super::verify;
use crate::host::State;
use dynet_core::{Error, Instance};
use dynet_host::Private;
use std::path::Path;
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(100);
const WINDOW: u64 = 3600;
const SETTLE: u64 = 120;

pub struct Arm<'a> {
    pub bootstrap: &'a Path,
    pub candidate: &'a Path,
    pub rollback: &'a Path,
    pub window: u64,
    pub settle: u64,
}

pub struct Guard {
    instance: String,
    vault: Vault,
}

impl Guard {
    pub fn new(instance: &Instance, state: &State, private: Private) -> Self {
        Self {
            instance: instance.get().to_owned(),
            vault: Vault::new(instance, state, private),
        }
    }

    pub fn arm(&self, arm: &Arm<'_>) -> Result<Record, Error> {
        if !(1..=WINDOW).contains(&arm.window) {
            return Err(Error::new(format!(
                "cutover window must be between 1 and {WINDOW} seconds"
            )));
        }
        if !(1..=SETTLE).contains(&arm.settle) {
            return Err(Error::new(format!(
                "cutover settle bound must be between 1 and {SETTLE} seconds"
            )));
        }
        let _lock = self.vault.lock()?;
        self.vault.clearable()?;
        let pair = verify::inspect(&self.vault, arm.bootstrap, arm.candidate, arm.rollback)?;
        self.vault.stage(CANDIDATE, &pair.proposed)?;
        self.vault.stage(ROLLBACK, &pair.backup)?;
        self.vault.reset();
        let record = Record {
            version: 1,
            id: token(),
            instance: self.instance.clone(),
            phase: Phase::Armed,
            bootstrap: arm.bootstrap.to_owned(),
            target: pair.target,
            window: arm.window,
            settle: arm.settle,
            boot: None,
            deadline: None,
            watcher: None,
            prior: pair.prior,
            candidate: pair.candidate,
            rollback: pair.rollback,
            reason: None,
        };
        self.vault.save(&record)?;
        Ok(record)
    }

    pub fn begin(&self, id: &str) -> Result<Record, Error> {
        let _lock = self.vault.lock()?;
        let mut record = self.vault.require(id)?;
        if record.phase != Phase::Armed {
            return Err(Error::new(format!(
                "cutover {id} is in {:?} state and cannot be applied",
                record.phase
            )));
        }
        let _ = self.vault.staged(CANDIDATE, &record.candidate.digest)?;
        let _ = self.vault.staged(ROLLBACK, &record.rollback.digest)?;
        let (boot, now) = clock()?;
        self.vault.reset();
        record.phase = Phase::Applying;
        record.boot = Some(boot);
        record.deadline = Some(now.saturating_add(record.window));
        record.watcher = None;
        record.reason = None;
        self.vault.save(&record)?;
        Ok(record)
    }

    pub fn abort(&self, id: &str, reason: &str) -> Result<(), Error> {
        let _lock = self.vault.lock()?;
        let mut record = self.vault.require(id)?;
        if record.phase == Phase::Applying {
            record.phase = Phase::Failed;
            record.reason = Some(reason.to_owned());
            self.vault.save(&record)?;
        }
        Ok(())
    }

    pub fn ready(&self, id: &str, pid: u32) -> Result<bool, Error> {
        let _lock = self.vault.lock()?;
        let Some(mut record) = self.vault.load()? else {
            return Ok(false);
        };
        if record.id != id || !matches!(record.phase, Phase::Applying | Phase::Active) {
            return Ok(false);
        }
        record.watcher = Some(pid);
        self.vault.save(&record)?;
        self.vault.mark(id, pid)?;
        Ok(true)
    }

    pub fn activate(&self, id: &str, settle: Option<u64>) -> Result<Record, Error> {
        let Some(pid) = self.vault.awaiter(id)? else {
            let message = "deadline worker did not become ready before candidate install";
            let _ = self.abort(id, message);
            return Err(Error::new(message));
        };
        let (record, expected, wait) = {
            let _lock = self.vault.lock()?;
            let mut record = self.vault.require(id)?;
            if record.phase != Phase::Applying {
                return Err(Error::new(format!(
                    "cutover {id} left applying state ({:?}) before candidate install",
                    record.phase
                )));
            }
            let bytes = self.vault.staged(CANDIDATE, &record.candidate.digest)?;
            self.vault.install(&record.target, &bytes)?;
            record.phase = Phase::Active;
            record.watcher = Some(pid);
            self.vault.save(&record)?;
            let expected = record.candidate.clone();
            let wait = settle.unwrap_or(record.settle);
            (record, expected, wait)
        };
        if wait > 0 {
            let _ = self.vault.converge(&expected, wait);
        }
        Ok(record)
    }

    pub fn confirm(&self, id: &str) -> Result<Record, Error> {
        let _lock = self.vault.lock()?;
        let mut record = self.vault.require(id)?;
        if record.phase != Phase::Active {
            return Err(Error::new(format!(
                "cutover {id} is in {:?} state and cannot be confirmed",
                record.phase
            )));
        }
        let (boot, now) = clock()?;
        if expired(&record, &boot, now) {
            return Err(Error::new(
                "cutover deadline expired; rollback owns recovery",
            ));
        }
        if !self.vault.applied(&record.candidate)? {
            return Err(Error::new(format!(
                "candidate generation {} is not applied in control checkpoint",
                record.candidate.generation
            )));
        }
        record.phase = Phase::Confirmed;
        record.reason = None;
        self.vault.save(&record)?;
        self.vault.clean();
        Ok(record)
    }

    pub fn rollback(&self, id: &str, settle: Option<u64>) -> Result<Record, Error> {
        let _lock = self.vault.lock()?;
        let mut record = self.vault.require(id)?;
        match record.phase {
            Phase::Armed => {
                record.phase = Phase::Reverted;
                record.reason = None;
                self.vault.save(&record)?;
                self.vault.clean();
                Ok(record)
            }
            Phase::Applying | Phase::Active => {
                let bound = settle.unwrap_or(record.settle);
                self.vault.revert(&mut record, bound)?;
                Ok(record)
            }
            other => Err(Error::new(format!(
                "cutover {id} is in {other:?} state and cannot be rolled back"
            ))),
        }
    }

    pub fn reconcile(&self, settle: Option<u64>) -> Result<Option<Record>, Error> {
        let _lock = self.vault.lock()?;
        let Some(mut record) = self.vault.load()? else {
            return Ok(None);
        };
        if matches!(record.phase, Phase::Applying | Phase::Active) {
            let (boot, now) = clock()?;
            if expired(&record, &boot, now) {
                let bound = settle.unwrap_or(record.settle);
                self.vault.revert(&mut record, bound)?;
            }
        }
        Ok(Some(record))
    }

    pub fn watch(&self, id: &str, settle: Option<u64>) -> Result<Option<Record>, Error> {
        if !self.ready(id, std::process::id())? {
            return Ok(None);
        }
        loop {
            if let Some(done) = self.step(id, settle)? {
                return Ok(done);
            }
            thread::sleep(POLL);
        }
    }

    pub fn status(&self) -> Result<Option<Record>, Error> {
        self.vault.load()
    }

    pub fn clear(&self, id: &str) -> Result<Record, Error> {
        let _lock = self.vault.lock()?;
        let record = self.vault.require(id)?;
        if !record.phase.retained() {
            return Err(Error::new(format!(
                "cutover {id} is in {:?} state; only failed or unknown state may be cleared",
                record.phase
            )));
        }
        self.vault.purge()?;
        Ok(record)
    }

    fn step(&self, id: &str, settle: Option<u64>) -> Result<Option<Option<Record>>, Error> {
        let _lock = self.vault.lock()?;
        let Some(mut record) = self.vault.load()? else {
            return Ok(Some(None));
        };
        if record.id != id || record.phase.terminal() || record.phase.retained() {
            return Ok(Some(Some(record)));
        }
        let (boot, now) = clock()?;
        if matches!(record.phase, Phase::Applying | Phase::Active) && expired(&record, &boot, now) {
            let bound = settle.unwrap_or(record.settle);
            self.vault.revert(&mut record, bound)?;
            return Ok(Some(Some(record)));
        }
        Ok(None)
    }
}

pub fn detach() -> Result<(), Error> {
    dynet_host::linux::detach()
        .map_err(|error| Error::new(format!("cannot detach deadline worker: {error}")))
}
