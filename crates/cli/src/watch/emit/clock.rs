use dynet_core::Error;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Instant;

pub(super) struct Clock {
    boot: u64,
    started: Instant,
}

impl Clock {
    pub(super) fn open() -> Result<Self, Error> {
        let text =
            fs::read_to_string("/proc/uptime").map_err(|error| Error::new(error.to_string()))?;
        let boot = text
            .split('.')
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::new("the uptime is unavailable"))?;
        Ok(Self {
            boot,
            started: Instant::now(),
        })
    }

    pub(super) fn now(&self) -> u64 {
        self.boot.saturating_add(self.started.elapsed().as_secs())
    }

    pub(super) fn deadline(&self, root: &Path) -> u64 {
        let file = root.join("detail.until");
        let read = || -> Result<u64, Error> {
            Self::private(root, true)?;
            Self::private(&file, false)?;
            let text = fs::read_to_string(file).map_err(|error| Error::new(error.to_string()))?;
            let until = text
                .trim()
                .parse::<u64>()
                .map_err(|_| Error::new("the diagnostic deadline is invalid"))?;
            match until <= self.now().saturating_add(300) {
                true => Ok(until),
                false => Err(Error::new("the diagnostic deadline exceeds five minutes")),
            }
        };
        read().unwrap_or(0)
    }

    pub(super) fn private(path: &Path, directory: bool) -> Result<(), Error> {
        let held = fs::symlink_metadata(path).map_err(|error| Error::new(error.to_string()))?;
        let owner = fs::metadata("/proc/self")
            .map_err(|error| Error::new(error.to_string()))?
            .uid();
        let wanted = if directory { 0o700 } else { 0o600 };
        let custody = held.uid() == owner && held.mode() & 0o777 == wanted;
        let kind = if directory {
            held.is_dir()
        } else {
            held.is_file() && held.nlink() == 1
        };
        if !custody || !kind || held.file_type().is_symlink() {
            return Err(Error::new(
                "diagnostic control requires a private owned path",
            ));
        }
        Ok(())
    }
}
