mod establish;
mod fragment;
mod link;
mod reclaim;
mod route;
mod shape;
mod veil;

pub use establish::{Ground, Standing, establish};
pub use fragment::{Fragment, Held, MARKER, held};
pub use link::Link;
pub use reclaim::{Cleared, reclaim, sweep};
pub use route::Route;
pub use shape::{Shape, read, survey};
pub use veil::Veil;

use dynet_core::Error;
use std::process::Command;

pub const PREFIX: &str = "198.51.100.0/24";

pub fn run(program: &str, arguments: &[&str]) -> Result<String, Error> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| Error::new(format!("cannot run {program}: {error}")))?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "{program} {} refused: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn attempt(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}
