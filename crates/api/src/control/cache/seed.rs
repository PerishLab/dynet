use super::{Cache, LIMIT, empty};
use crate::control::{Candidate, custody, refused};
use dynet_core::{Error, Ledger};
use secrets::Kind;
use serde_json::value::RawValue;

impl Cache {
    pub(in crate::control) fn seed(
        &self,
        bytes: &[u8],
        candidate: &Candidate,
        ledger: &Ledger,
    ) -> Result<(), Error> {
        let mut checkpoint = self.lock()?;
        empty(&self.root)?;
        if checkpoint.received.is_some() || candidate.payload.is_none() {
            return Err(refused());
        }
        if candidate.assignment.kind != Kind::Configuration {
            return Err(refused());
        }
        let mut next = checkpoint.clone();
        next.received = Some(candidate.assignment.clone());
        next.validated = Some(candidate.assignment.receipt.clone());
        next.ledger = RawValue::from_string(ledger.write()).map_err(|_| refused())?;
        next.check(&checkpoint.trust)?;
        if serde_json::to_vec(&next).map_err(|_| refused())?.len() > LIMIT {
            return Err(refused());
        }
        self.private
            .create(&self.root.join("initialized"), b"dynet-control-v1")
            .map_err(custody)?;
        self.private
            .create(&self.blob(&candidate.assignment.receipt), bytes)
            .map_err(custody)?;
        self.commit(&mut checkpoint, next)
    }
}
