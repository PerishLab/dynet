use dynet_core::{Error, Instance};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn kept() -> PathBuf {
    PathBuf::from("/Library/Application Support/Dynet")
}

pub fn configure(root: &Path) -> Result<(), Error> {
    if let Some(prior) = ROOT.get() {
        return match prior == root {
            true => Ok(()),
            false => Err(Error::new(
                "the process already owns a different state root",
            )),
        };
    }
    ROOT.set(root.to_path_buf())
        .map_err(|_| Error::new("the state root changed concurrently"))
}

pub(super) fn path(instance: &Instance, extension: &str) -> PathBuf {
    ROOT.get()
        .cloned()
        .unwrap_or_else(kept)
        .join(format!("{}.{}", instance.get(), extension))
}
