mod custody;
mod dart;
mod divert;
mod link;
pub mod sniff;
pub mod standing;
mod store;
mod warden;
mod warren;

use std::time::{Duration, Instant};

pub(crate) const IDLE: Duration = Duration::from_secs(30);

pub(crate) fn lasting(started: Instant, span: Duration) -> bool {
    span.is_zero() || started.elapsed() < span
}

pub use divert::{divert, observe};
pub use link::{Link, Taken};
pub use store::Store;
use warren::evidence;
pub use warren::evidence::{Evidence, Reporter};
pub use warren::{Charge, Held, Served, Warren};
