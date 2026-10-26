use dynet_core::Error;
use std::path::PathBuf;

pub fn kept() -> PathBuf {
    let data = std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(data).join("dynet")
}

pub fn program() -> Result<PathBuf, Error> {
    std::env::current_exe()
        .map_err(|error| Error::new(format!("cannot name this program: {error}")))
}
