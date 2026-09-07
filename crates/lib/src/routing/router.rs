use super::domain::Domain;
use super::ledger::Ledger;
use super::table::{Decision, Table};
use std::net::IpAddr;
use std::time::Instant;

#[derive(Debug)]
pub struct Router {
    table: Table,
    ledger: Ledger,
}

impl Router {
    pub fn new(table: Table) -> Self {
        Self {
            table,
            ledger: Ledger::new(),
        }
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    pub fn relay(&mut self, table: Table) {
        self.table = table;
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub fn asked(&self, domain: &Domain) -> Decision {
        self.table.named(domain)
    }

    pub fn issued(
        &mut self,
        domain: &Domain,
        answers: &[IpAddr],
        decision: &Decision,
        expiry: Instant,
    ) {
        for address in answers {
            let issue = Ledger::issue(domain.clone(), decision.clone(), expiry);
            self.ledger.record(*address, issue);
        }
    }

    pub fn forget(&mut self, now: Instant) -> Vec<IpAddr> {
        self.ledger.sweep(now)
    }

    pub fn reached(&self, address: IpAddr, now: Instant) -> Decision {
        match self.ledger.lookup(address, now) {
            Some(issue) => issue.decision().clone(),
            None => self.table.addressed(address),
        }
    }
}
