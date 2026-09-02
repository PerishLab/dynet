use dynet_core::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub const MARKER: &str = "# managed by dynet; remove this line and dynet will not touch the file";

pub struct Fragment {
    path: PathBuf,
    body: String,
}

impl Fragment {
    pub fn new(path: impl Into<PathBuf>, body: impl AsRef<str>) -> Self {
        Self {
            path: path.into(),
            body: format!("{MARKER}\n{}", body.as_ref()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                Error::new(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        if held(&self.path)? == Held::Foreign {
            return Err(Error::new(format!(
                "{} exists without the dynet marker and is the operator's",
                self.path.display()
            )));
        }
        fs::write(&self.path, &self.body)
            .map_err(|error| Error::new(format!("cannot write {}: {error}", self.path.display())))
    }

    pub fn remove(path: &Path) -> Result<bool, Error> {
        match held(path)? {
            Held::Absent => Ok(false),
            Held::Foreign => Err(Error::new(format!(
                "{} exists without the dynet marker and was left alone",
                path.display()
            ))),
            Held::Owned => {
                fs::remove_file(path).map_err(|error| {
                    Error::new(format!("cannot remove {}: {error}", path.display()))
                })?;
                Ok(true)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Held {
    Absent,
    Owned,
    Foreign,
}

pub fn held(path: &Path) -> Result<Held, Error> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Held::Absent),
        Err(error) => Err(Error::new(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
        Ok(text) if text.contains(MARKER) => Ok(Held::Owned),
        Ok(_) => Ok(Held::Foreign),
    }
}
