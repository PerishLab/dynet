use super::refused;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

const STICKY: u32 = 0o1000;

pub(super) struct Place<'a>(pub &'a Path);

impl Place<'_> {
    pub(super) fn open(&self, directory: bool) -> io::Result<File> {
        let flags =
            libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 };
        OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(self.0)
    }

    pub(super) fn create(&self, machine: bool) -> io::Result<File> {
        if machine && unsafe { libc::geteuid() } != 0 {
            return Err(refused());
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.0)
    }

    pub(super) fn folder(&self, machine: bool) -> io::Result<()> {
        if machine && unsafe { libc::geteuid() } != 0 {
            return Err(refused());
        }
        std::fs::DirBuilder::new().mode(0o700).create(self.0)
    }

    pub(super) fn sync(&self) -> io::Result<()> {
        File::open(self.0.parent().ok_or_else(refused)?)?.sync_all()
    }
}

pub(super) fn check(file: &File, machine: bool, ancestor: bool) -> io::Result<()> {
    let meta = file.metadata()?;
    let uid = unsafe { libc::geteuid() };
    if meta.uid() != 0 && (machine || meta.uid() != uid) {
        return Err(refused());
    }
    if ancestor {
        let sticky = meta.mode() & STICKY != 0;
        if !meta.is_dir() || (meta.mode() & 0o022 != 0 && !sticky) {
            return Err(refused());
        }
    } else {
        if meta.mode() & 0o077 != 0 {
            return Err(refused());
        }
        match (meta.is_dir(), meta.is_file(), meta.nlink()) {
            (true, _, _) | (false, true, 1) => (),
            _ => return Err(refused()),
        }
    }
    Ok(())
}

pub(super) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}
