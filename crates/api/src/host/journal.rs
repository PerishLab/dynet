use crate::control::Cache;
use dynet_core::{Error, Instance, Ledger};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Journal {
    path: PathBuf,
    order: PathBuf,
    stained: AtomicBool,
    cache: Option<Arc<Cache>>,
}

impl Journal {
    pub fn new(instance: &Instance, state: &super::State) -> Self {
        let name = instance.get();
        Self {
            path: state.root().join(format!("{name}.control/checkpoint")),
            order: state.runtime().join(format!("{name}.flush")),
            stained: AtomicBool::new(false),
            cache: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn order(&self) -> &Path {
        &self.order
    }

    pub fn standing(&self) -> PathBuf {
        self.order.with_extension("standing")
    }

    pub fn answer(&self) -> PathBuf {
        self.order.with_extension("flushed")
    }

    pub fn controlled(instance: &Instance, state: &super::State, cache: Arc<Cache>) -> Self {
        let mut journal = Self::new(instance, state);
        journal.cache = Some(cache);
        journal
    }

    pub fn load(&self, floor: u64) -> Result<Ledger, Error> {
        self.cache
            .as_ref()
            .ok_or_else(unbound)?
            .checkpoint()?
            .ledger(floor)
    }

    pub fn save(&self, ledger: &Ledger) -> Result<(), Error> {
        self.cache.as_ref().ok_or_else(unbound)?.save(ledger)
    }

    pub fn stain(&self) {
        self.stained.store(true, Ordering::Relaxed);
    }

    pub fn stained(&self) -> bool {
        self.stained.swap(false, Ordering::Relaxed)
    }

    pub fn taken(&self) -> Option<String> {
        let text = fs::read_to_string(&self.order).ok()?;
        let _ = fs::remove_file(&self.order);
        Some(text.trim().to_string())
    }

    pub fn reply(&self, text: &str) -> Result<(), Error> {
        place(&self.answer(), text)
    }

    pub fn ask(&self, text: &str) -> Result<(), Error> {
        let _ = fs::remove_file(self.answer());
        place(&self.order, text)
    }
}

pub fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn place(path: &Path, text: &str) -> Result<(), Error> {
    let failed =
        |error: std::io::Error| Error::new(format!("cannot write {}: {error}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    let part = path.with_extension("part");
    fs::write(&part, text).map_err(failed)?;
    fs::rename(&part, path).map_err(failed)
}

fn unbound() -> Error {
    Error::new("the journal has no encrypted configuration checkpoint")
}
