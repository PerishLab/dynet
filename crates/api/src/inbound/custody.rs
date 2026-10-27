use super::warren::{Held, Warren};
use crate::control::Candidate;
use crate::host::{Route, clock};
use dynet_core::{Error, Ledger, Range, Sieve, Subject, Tally};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::MutexGuard;
use std::sync::atomic::Ordering;

impl Warren<'_> {
    pub fn confirmed(&self) -> Result<(), Error> {
        let ledger = self.lock()?;
        let checkpoint = self.control.cache.checkpoint()?;
        if checkpoint.applied.as_ref() == Some(self.initial) {
            self.control.cache.save(&ledger)?;
        } else {
            self.control.cache.apply(self.initial, &ledger)?;
        }
        self.control.phase(
            if self.current().paused {
                secrets::Phase::Revoked
            } else {
                secrets::Phase::Applied
            },
            None,
        );
        Ok(())
    }

    pub fn renew(&self) {
        if self.control.failed() {
            let _ = self.current().fence.close();
            self.halt.store(true, Ordering::Release);
            return;
        }
        if let Err(error) = self.control.reconcile(|candidate| self.apply(candidate)) {
            (self.told)(&format!(
                "configuration application refused; confirmed state retained: {error}"
            ));
        }
    }

    pub fn apply(&self, candidate: &Candidate) -> Result<(), Error> {
        let mut ledger = self.lock()?;
        let mut slot = self
            .held
            .write()
            .map_err(|_| Error::new("the policy is poisoned"))?;
        let mut next = match candidate.payload.clone() {
            Some(payload) => Held::new(payload, Some(&slot))?,
            None => {
                slot.fence.close().inspect_err(|_| {
                    self.halt.store(true, Ordering::Release);
                })?;
                let payload = crate::control::Payload {
                    loaded: slot.loaded.clone(),
                    upstream: slot
                        .upstream
                        .parse()
                        .map_err(|_| Error::new("the upstream is invalid"))?,
                    retain: slot.retain,
                };
                let mut held = Held::new(payload, Some(&slot))?;
                held.fence.close()?;
                held.paused = true;
                held
            }
        };
        next.stamp = candidate.assignment.receipt.digest.clone();
        let mut issued = ledger.clone();
        issued.retention(next.retain);
        if !next.paused {
            issued.rejudge(next.loaded.manifest());
        }
        let retained = wanted(&slot, &ledger);
        let projected = wanted(&next, &issued);
        let guarded = retained.union(&projected).copied().collect();
        self.projection.settle(&guarded)?;
        if let Err(error) = self.control.cache.apply(&candidate.assignment, &issued) {
            let _ = slot.fence.close();
            let _ = next.fence.close();
            self.halt.store(true, Ordering::Release);
            return Err(error);
        }
        if next.paused {
            next.pools.clear();
        }
        *slot = Arc::new(next);
        *ledger = issued;
        self.store.clear();
        self.book.clear();
        drop(slot);
        drop(ledger);
        if let Err(error) = self.projection.settle(&projected) {
            (self.told)(&format!(
                "old protective routes retained after configuration commit: {error}"
            ));
        }
        (self.told)("encrypted configuration confirmed");
        Ok(())
    }

    pub fn forget(&self, engaged: &BTreeMap<IpAddr, u32>) {
        let Ok(mut ledger) = self.ledger.lock() else {
            return;
        };
        ledger.engage(engaged);
        if self.current().paused {
            return;
        }
        let gone = ledger.sweep(clock());
        if gone.is_empty() {
            return;
        }
        self.journal.stain();
        (self.told)(&format!("released {} retained addresses", gone.len()));
        self.project(&ledger);
    }

    pub fn settle(&self) -> Result<(), Error> {
        let ledger = self.lock()?;
        let wanted = self.wanted(&ledger);
        let settled = Route::settle(self.instance, &wanted)?;
        (self.told)(&format!(
            "settled {} issued addresses and held ranges, {} added, {} dropped",
            wanted.len(),
            settled.added,
            settled.dropped
        ));
        Ok(())
    }

    pub fn rejudged(&self) {
        let held = self.current();
        let Ok(mut ledger) = self.ledger.lock() else {
            return;
        };
        let tally = ledger.rejudge(held.loaded.manifest());
        if !tally.touched() {
            return;
        }
        self.journal.stain();
        (self.told)(&format!("ledger re-judged {}", spoken(&tally)));
        self.project(&ledger);
    }

    pub fn heed(&self) {
        if let Some(order) = self.journal.taken() {
            let reply = self.flush(&order);
            if let Err(error) = self.journal.reply(&reply) {
                (self.told)(&format!("cannot answer the flush order: {error}"));
            }
        }
        if !self.journal.stained() {
            return;
        }
        let Ok(ledger) = self.ledger.lock() else {
            return;
        };
        if let Err(error) = self.journal.save(&ledger) {
            self.journal.stain();
            (self.told)(&format!("the ledger was not saved: {error}"));
            let _ = self.current().fence.close();
            self.halt.store(true, Ordering::Release);
        }
    }

    fn flush(&self, order: &str) -> String {
        if self.current().paused {
            return "refused: the signed configuration is revoked".to_owned();
        }
        let (told, dry) = match order.strip_suffix(" dry") {
            Some(told) => (told.trim(), true),
            None => (order, false),
        };
        let Some(sieve) = Sieve::read(told) else {
            return format!("refused: {told} is no name, address or prefix");
        };
        let held = self.current();
        let Ok(mut ledger) = self.ledger.lock() else {
            return "refused: the ledger is poisoned".to_string();
        };
        let tally = ledger.flush(held.loaded.manifest(), &sieve, dry);
        let line = format!("{} {told}: {}", heading(dry), spoken(&tally));
        (self.told)(&line);
        if !dry && tally.touched() {
            self.journal.stain();
            self.project(&ledger);
        }
        line
    }

    fn wanted(&self, ledger: &Ledger) -> BTreeSet<Range> {
        wanted(&self.current(), ledger)
    }

    fn project(&self, ledger: &Ledger) {
        let wanted = self.wanted(ledger);
        if let Err(error) = Route::settle(self.instance, &wanted) {
            (self.told)(&format!("settle failed: {error}"));
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Ledger>, Error> {
        self.ledger
            .lock()
            .map_err(|_| Error::new("the ledger is poisoned"))
    }
}

fn heading(dry: bool) -> &'static str {
    match dry {
        true => "would flush",
        false => "flushed",
    }
}

fn spoken(tally: &Tally) -> String {
    format!(
        "moved={} released={} kept={} crowded={}",
        tally.moved, tally.released, tally.kept, tally.crowded
    )
}

fn wanted(held: &Held, ledger: &Ledger) -> BTreeSet<Range> {
    let ranges = held
        .loaded
        .manifest()
        .table()
        .rules()
        .iter()
        .filter_map(|rule| match rule.subject() {
            Subject::Holds(range) => Some(range.masked()),
            _ => None,
        });
    ledger.addresses().map(Range::host).chain(ranges).collect()
}
