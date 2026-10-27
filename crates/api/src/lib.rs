#![forbid(unsafe_code)]

pub mod control;
pub mod host;
pub mod inbound;
pub mod manifest;
pub mod outbound;
pub mod resolver;

pub use manifest::{Entry, Loaded};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
