mod establish;
mod link;
mod reclaim;
mod route;
mod settle;
mod shape;
mod veil;

pub use establish::{Ground, Standing, establish};
pub use link::Link;
pub use reclaim::{Cleared, reclaim, sweep};
pub use route::Route;
pub use settle::Settled;
pub use shape::{Shape, read, survey};
pub use veil::Veil;

use std::path::PathBuf;

pub fn kept() -> PathBuf {
    PathBuf::from("/var/lib/dynet")
}

pub fn passing() -> PathBuf {
    PathBuf::from("/run/dynet")
}
