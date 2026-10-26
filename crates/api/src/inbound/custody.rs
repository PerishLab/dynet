use super::warren::Warren;
use crate::host::{Route, clock};
use dynet_core::{Error, Ledger, Range, Sieve, Subject, Tally};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::MutexGuard;

impl Warren<'_> {
    pub fn forget(&self, engaged: &BTreeMap<IpAddr, u32>) {
        let Ok(mut ledger) = self.ledger.lock() else {
            return;
        };
        ledger.engage(engaged);
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
        }
    }

    fn flush(&self, order: &str) -> String {
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
        let held = self.current();
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
