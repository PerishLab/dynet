#![forbid(unsafe_code)]

pub mod catalog;
pub mod host;
pub mod outbound;
pub mod resolver;
pub mod subscription;

pub use catalog::{Catalog, Declined, Reason};
pub use subscription::Entry;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
