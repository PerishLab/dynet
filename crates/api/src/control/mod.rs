mod bootstrap;
mod cache;
mod candidate;
mod network;
mod payload;
mod projection;
mod session;
mod worker;

pub use bootstrap::{Bootstrap, Source};
pub use cache::{Cache, Checkpoint};
pub use candidate::{Candidate, Decoder, Incoming};
pub use dynet_host::Private;
pub use payload::Payload;
pub use projection::Projection;
pub use session::{Control, Session};

use dynet_core::Error;

fn refused() -> Error {
    Error::new("encrypted configuration refused")
}

fn custody(_: std::io::Error) -> Error {
    Error::new("private control state refused")
}

#[cfg(test)]
mod tests;

pub fn identity(path: &std::path::Path, private: Private) -> Result<String, Error> {
    use secrets::ExposeSecret;
    let identity = secrets::Identity::generate();
    private
        .create(path, identity.export().expose_secret().as_bytes())
        .map_err(custody)?;
    Ok(identity.recipient().to_string())
}
