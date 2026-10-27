#[cfg(windows)]
mod acl;
#[cfg(windows)]
mod descriptor;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use unix as native;
#[cfg(windows)]
use windows as native;

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct Private {
    machine: bool,
}

impl Private {
    pub fn current() -> Self {
        Self { machine: false }
    }

    pub fn machine() -> Self {
        Self { machine: true }
    }

    pub fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        self.ancestors(path)?;
        let file = native::Place(path).open(false)?;
        native::check(&file, self.machine, false)?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(refused());
        }
        Ok(bytes)
    }

    pub fn folder(&self, path: &Path) -> io::Result<()> {
        self.ancestors(path)?;
        match native::Place(path).folder(self.machine) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
        native::check(&native::Place(path).open(true)?, self.machine, false)
    }

    pub fn create(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.ancestors(path)?;
        let mut file = native::Place(path).create(self.machine)?;
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        if written.is_err() {
            drop(file);
            let _ = std::fs::remove_file(path);
        }
        written?;
        native::Place(path).sync()
    }

    pub fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.ancestors(path)?;
        let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
        let part = path.with_extension(format!("part-{}-{ordinal}", std::process::id()));
        self.create(&part, bytes)?;
        let moved = native::replace(&part, path).and_then(|()| native::Place(path).sync());
        if moved.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        moved
    }

    pub fn lock(&self, path: &Path) -> io::Result<File> {
        self.ancestors(path)?;
        let file = match native::Place(path).create(self.machine) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                native::Place(path).open(false)?
            }
            Err(error) => return Err(error),
        };
        native::check(&file, self.machine, false)?;
        file.try_lock().map_err(io::Error::other)?;
        Ok(file)
    }

    fn ancestors(&self, path: &Path) -> io::Result<()> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(refused());
        }
        let parent = path.parent().ok_or_else(refused)?;
        for ancestor in parent.ancestors() {
            native::check(&native::Place(ancestor).open(true)?, self.machine, true)?;
        }
        Ok(())
    }
}

fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "private custody refused")
}
