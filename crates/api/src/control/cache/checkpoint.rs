use super::super::refused;
use dynet_core::{Error, Ledger};
use secrets::{Assignment, Kind, Receipt};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub(super) version: u8,
    pub(super) trust: String,
    pub received: Option<Assignment>,
    pub validated: Option<Receipt>,
    pub applied: Option<Assignment>,
    pub retained: Option<Assignment>,
    pub barred: Option<Assignment>,
    pub(super) ledger: Box<RawValue>,
}

impl Checkpoint {
    pub(super) fn check(&self, trust: &str) -> Result<(), Error> {
        if self.version != 1 || self.trust != trust {
            return Err(refused());
        }
        for assignment in [&self.received, &self.applied, &self.retained, &self.barred]
            .into_iter()
            .flatten()
        {
            assignment.check().map_err(|_| refused())?;
        }
        let Some(received) = &self.received else {
            return if self.receipts().next().is_some() {
                Err(refused())
            } else {
                Ok(())
            };
        };
        for receipt in self.receipts() {
            received.receipt.follows(receipt).map_err(|_| refused())?;
        }
        for assignment in [&self.applied, &self.retained, &self.barred]
            .into_iter()
            .flatten()
        {
            if assignment.target != received.target {
                return Err(refused());
            }
        }
        if let Some(applied) = &self.applied {
            self.validated
                .as_ref()
                .ok_or_else(refused)?
                .follows(&applied.receipt)
                .map_err(|_| refused())?;
            if applied.kind == Kind::Configuration && self.retained.as_ref() != Some(applied) {
                return Err(refused());
            }
        }
        if let Some(barred) = &self.barred {
            if barred.kind != Kind::Revocation {
                return Err(refused());
            }
            received
                .receipt
                .follows(&barred.receipt)
                .map_err(|_| refused())?;
            if self.applied.as_ref().is_some_and(|applied| {
                applied.kind == Kind::Configuration
                    && applied.receipt.generation >= barred.receipt.generation
            }) {
                return Err(refused());
            }
        }
        if let Some(retained) = &self.retained {
            if retained.kind != Kind::Configuration {
                return Err(refused());
            }
            let applied = self.applied.as_ref().ok_or_else(refused)?;
            applied
                .receipt
                .follows(&retained.receipt)
                .map_err(|_| refused())?;
        }
        Ok(())
    }

    fn receipts(&self) -> impl Iterator<Item = &Receipt> {
        [
            self.validated.as_ref(),
            self.applied.as_ref().map(|assignment| &assignment.receipt),
            self.retained.as_ref().map(|assignment| &assignment.receipt),
            self.barred.as_ref().map(|assignment| &assignment.receipt),
        ]
        .into_iter()
        .flatten()
    }

    pub fn ledger(&self, retain: u64) -> Result<Ledger, Error> {
        Ledger::read(self.ledger.get(), retain).map_err(|_| refused())
    }
}
