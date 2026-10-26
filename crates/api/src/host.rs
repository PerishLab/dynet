mod fragment;
mod journal;
#[cfg(not(windows))]
mod linux;
mod state;
#[cfg(windows)]
mod windows;

pub use fragment::{Fragment, Held, MARKER, held};
pub use journal::{Journal, clock};
#[cfg(not(windows))]
pub use linux::{
    Cleared, Ground, Link, Route, Settled, Shape, Standing, Veil, establish, read, reclaim, survey,
    sweep,
};
pub use state::State;
#[cfg(windows)]
pub use windows::{
    Cleared, Ground, Link, Route, Settled, Standing, Veil, establish, program, reclaim,
};

use dynet_core::Error;
use std::io::Write;
use std::process::{Command, Stdio};

pub const PREFIX: &str = "198.51.100.0/24";

pub fn run(program: &str, arguments: &[&str]) -> Result<String, Error> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| Error::new(format!("cannot run {program}: {error}")))?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "{program} {} refused: {}{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout).trim(),
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

pub fn feed(program: &str, arguments: &[&str], input: &str) -> Result<(), Error> {
    let refused = |error: std::io::Error| Error::new(format!("cannot run {program}: {error}"));
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(refused)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).map_err(refused)?;
    }
    let output = child.wait_with_output().map_err(refused)?;
    match output.status.success() {
        true => Ok(()),
        false => Err(Error::new(format!(
            "{program} {} refused: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}
