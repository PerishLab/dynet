#![forbid(unsafe_code)]

mod capability;
mod cluster;
mod error;
mod instance;
mod label;
mod pool;
mod routing;

pub use capability::{Capability, Carriage};
pub use cluster::{Cluster, Name, Node};
pub use error::Error;
pub use instance::Instance;
pub use label::Label;
pub use pool::{Affinity, Pool, Spread};
pub use routing::{Decision, Domain, Ground, Issue, Ledger, Range, Router, Rule, Subject, Table};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
