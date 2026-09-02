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
    contested: usize,
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
        let crossed = self
            .issued
            .get(&address)
            .is_some_and(|held| held.decision().cluster() != issue.decision.cluster());
        if crossed {
            self.contested += 1;
        }
        self.issued.insert(address, issue);
    }

    pub fn contested(&self) -> usize {
        self.contested
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

    pub fn sweep(&mut self, now: Instant) -> Vec<IpAddr> {
        let gone: Vec<IpAddr> = self
            .issued
            .iter()
            .filter(|(_, issue)| issue.expiry <= now)
            .map(|(address, _)| *address)
            .collect();
        self.issued.retain(|_, issue| issue.expiry > now);
        gone
    }

    pub fn empty(&self) -> bool {
        self.issued.is_empty()
    }
}
