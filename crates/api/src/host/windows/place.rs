use dynet_core::Error;
use std::path::PathBuf;

pub fn kept() -> Result<PathBuf, Error> {
    dynet_host::windows::data()
        .map(|root| root.join("dynet"))
        .map_err(|error| Error::new(format!("cannot place local state: {error}")))
}

pub fn program() -> Result<PathBuf, Error> {
    std::env::current_exe()
        .map_err(|error| Error::new(format!("cannot name this program: {error}")))
}
