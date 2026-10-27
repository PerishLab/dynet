use dynet_core::Error;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct State {
    root: PathBuf,
    runtime: PathBuf,
    explicit: bool,
}

impl State {
    pub fn new(root: Option<PathBuf>) -> Result<Self, Error> {
        let Some(root) = root else {
            return Self::defaulted();
        };
        let root = std::path::absolute(root)
            .map_err(|error| Error::new(format!("cannot place local state: {error}")))?;
        Ok(Self {
            runtime: root.join("run"),
            root,
            explicit: true,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn runtime(&self) -> &Path {
        &self.runtime
    }

    pub fn configured(&self) -> Option<&Path> {
        self.explicit.then_some(&self.root)
    }

    #[cfg(windows)]
    fn defaulted() -> Result<Self, Error> {
        let root = super::windows::kept()?;
        Ok(Self {
            runtime: root.join("run"),
            root,
            explicit: false,
        })
    }

    #[cfg(target_os = "macos")]
    fn defaulted() -> Result<Self, Error> {
        let root = super::macos::kept();
        Ok(Self {
            runtime: root.join("run"),
            root,
            explicit: false,
        })
    }

    #[cfg(target_os = "linux")]
    fn defaulted() -> Result<Self, Error> {
        Ok(Self {
            root: super::linux::kept(),
            runtime: super::linux::passing(),
            explicit: false,
        })
    }
}
