#![forbid(unsafe_code)]

mod error;
mod instance;
mod manifest;
mod pool;
mod routing;

pub use error::Error;
pub use instance::Instance;
pub use manifest::{
    Bearing, Capability, Carriage, Cluster, DIRECT, Declared, Form, Label, Manifest, Name, Node,
};
pub use pool::{Affinity, Blame, Fault, Policy, Selector, Standing, Verdict};
pub use routing::{
    Course, Decision, Domain, Ground, Issue, Ledger, Range, Refusal, Rule, Sight, Span, Subject,
    Table, Told, decide, reserved, spared, usable,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
