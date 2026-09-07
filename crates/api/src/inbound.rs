mod dart;
mod divert;
mod link;
pub mod standing;
mod store;
pub mod table;
mod warden;
mod warren;

use std::time::{Duration, Instant};

pub(crate) const IDLE: Duration = Duration::from_secs(30);

pub(crate) fn lasting(started: Instant, span: Duration) -> bool {
    span.is_zero() || started.elapsed() < span
}

pub use divert::divert;
pub use link::{Link, Taken};
pub use store::Store;
pub use warren::{Charge, Served, Warren};
