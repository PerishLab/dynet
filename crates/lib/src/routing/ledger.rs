use super::domain::Domain;
use super::table::Decision;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Issue {
    domain: Domain,
    decision: Decision,
    expiry: Instant,
}

#[derive(Debug, Default)]
pub struct Ledger {
    issued: HashMap<IpAddr, Issue>,
}

impl Issue {
    pub fn domain(&self) -> &Domain {
        &self.domain
    }

    pub fn decision(&self) -> &Decision {
        &self.decision
    }

    pub fn expiry(&self) -> Instant {
        self.expiry
    }
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, address: IpAddr, issue: Issue) {
        self.issued.insert(address, issue);
    }

    pub fn issue(domain: Domain, decision: Decision, expiry: Instant) -> Issue {
        Issue {
            domain,
            decision,
            expiry,
        }
    }

    pub fn lookup(&self, address: IpAddr, now: Instant) -> Option<&Issue> {
        self.issued.get(&address).filter(|issue| issue.expiry > now)
    }

    pub fn sweep(&mut self, now: Instant) {
        self.issued.retain(|_, issue| issue.expiry > now);
    }

    pub fn empty(&self) -> bool {
        self.issued.is_empty()
    }
}
