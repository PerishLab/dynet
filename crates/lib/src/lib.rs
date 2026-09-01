#![forbid(unsafe_code)]

mod capability;
mod cluster;
mod error;
mod pool;

pub use capability::{Capability, Carriage};
pub use cluster::{Cluster, Name, Node};
pub use error::Error;
pub use pool::{Affinity, Pool, Spread};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
