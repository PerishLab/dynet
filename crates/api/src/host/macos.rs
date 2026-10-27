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
pub(super) fn refused(error: std::io::Error) -> Error {
    Error::new(format!("the macOS host refused: {error}"))
}
