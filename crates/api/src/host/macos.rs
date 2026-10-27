mod dns;
mod establish;
mod paths;
mod route;
mod wall;

pub use establish::{establish, reclaim};
pub use paths::{configure, kept};
pub use route::{Link, Route, Settled};
pub use wall::Veil;

use dynet_core::Error;
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static UPLINK: Mutex<Option<(Instant, u32)>> = Mutex::new(None);

pub fn uplink() -> std::io::Result<NonZeroU32> {
    let mut held = UPLINK
        .lock()
        .map_err(|_| std::io::Error::other("the uplink lock is poisoned"))?;
    if let Some((when, index)) = *held
        && when.elapsed() < Duration::from_secs(5)
    {
        return NonZeroU32::new(index).ok_or_else(|| std::io::Error::other("no physical uplink"));
    }
    let output = super::run("/sbin/route", &["-n", "get", "default"])
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let name = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface: "))
        .ok_or_else(|| std::io::Error::other("the default route names no interface"))?;
    if name.starts_with("utun") || name == "lo0" {
        return Err(std::io::Error::other(
            "the default route is not a physical uplink",
        ));
    }
    let index = dynet_tun::index(name)?;
    *held = Some((Instant::now(), index));
    NonZeroU32::new(index).ok_or_else(|| std::io::Error::other("no physical uplink"))
}

pub(super) fn refused(error: std::io::Error) -> Error {
    Error::new(format!("the macOS host refused: {error}"))
}
