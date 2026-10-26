use dynet_core::{Error, Instance, Ledger};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Journal {
    path: PathBuf,
    order: PathBuf,
    stained: AtomicBool,
}

impl Journal {
    pub fn new(instance: &Instance, state: &super::State, path: Option<PathBuf>) -> Self {
        let name = instance.get();
        Self {
            path: path.unwrap_or_else(|| state.root().join(format!("{name}.ledger"))),
            order: state.runtime().join(format!("{name}.flush")),
            stained: AtomicBool::new(false),
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

    pub fn load(&self, floor: u64) -> Result<Ledger, Error> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ledger::read(&text, floor).map_err(|error| {
                Error::new(format!(
                    "{} is unreadable, so dynet refuses to start rather than release issued addresses; remove it by hand to accept that: {error}",
                    self.path.display()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::new(floor)),
            Err(error) => Err(Error::new(format!(
                "cannot read {}: {error}",
                self.path.display()
            ))),
        }
    }

    pub fn save(&self, ledger: &Ledger) -> Result<(), Error> {
        place(&self.path, &ledger.write())
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
