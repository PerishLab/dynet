use super::{Bootstrap, Source};
use crate::control::{Cache, Candidate, Decoder, Private, custody, refused};
use crate::host::State;
use dynet_core::{Error, Instance, Ledger};
use secrets::{Assignment, Receipt};
use std::path::Path;

const LIMIT: usize = 16 * 1024 * 1024;

pub struct Seed<'a> {
    pub bootstrap: &'a Path,
    pub issued: &'a Path,
    pub instance: &'a Instance,
    pub state: &'a State,
}

impl Seed<'_> {
    pub fn run(&self, private: Private) -> Result<Receipt, Error> {
        let root = self
            .state
            .root()
            .join(format!("{}.control", self.instance.get()));
        match std::fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(Error::new("checkpoint destination must be absent")),
        }
        let bootstrap = Bootstrap::read(self.bootstrap, private)?;
        let identity = bootstrap.identity(private)?;
        let trust = bootstrap.trust(&identity)?;
        let decoder = Decoder::new(bootstrap.reader()?, identity);
        let (bytes, expected) = fetch(&bootstrap, private, self.instance.mark())?;
        let incoming = decoder.receive(bytes.clone(), None)?;
        if expected.is_some_and(|assignment| assignment != incoming.assignment) {
            return Err(refused());
        }
        let candidate = decoder.open(incoming)?;
        let ledger = self.ledger(&candidate, private)?;
        private.folder(self.state.root()).map_err(custody)?;
        private.fresh(&root).map_err(custody)?;
        let cache = Cache::open(&root, trust, private)?;
        cache.seed(&bytes, &candidate, &ledger)?;
        Ok(candidate.assignment.receipt)
    }

    fn ledger(&self, candidate: &Candidate, private: Private) -> Result<Ledger, Error> {
        let payload = candidate.payload.as_ref().ok_or_else(refused)?;
        let bytes = private.read(self.issued, LIMIT).map_err(custody)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| refused())?;
        let ledger = Ledger::read(text, payload.retain).map_err(|_| refused())?;
        let supplied: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        let retained: serde_json::Value =
            serde_json::from_str(&ledger.write()).map_err(|_| refused())?;
        if supplied != retained {
            return Err(Error::new(
                "issued ledger must preserve its native records exactly",
            ));
        }
        Ok(ledger)
    }
}

fn fetch(
    bootstrap: &Bootstrap,
    private: Private,
    mark: u32,
) -> Result<(Vec<u8>, Option<Assignment>), Error> {
    match &bootstrap.source {
        Source::File { path } => Ok((private.read(path, secrets::LIMIT).map_err(custody)?, None)),
        Source::Url { .. } => {
            let client = bootstrap.client(private, mark)?.ok_or_else(refused)?;
            let assignment = client
                .assignment(&bootstrap.target.device, None)
                .map_err(|_| refused())?
                .ok_or_else(refused)?;
            let bytes = client.fetch(&assignment).map_err(|_| refused())?;
            Ok((bytes, Some(assignment)))
        }
    }
}
