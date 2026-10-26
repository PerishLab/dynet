use super::domain::Domain;
use super::reserved::spared;
use crate::Name;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Issue {
    domain: Domain,
    cluster: Name,
    expiry: Instant,
}

#[derive(Debug, Default)]
pub struct Ledger {
    issued: HashMap<IpAddr, Issue>,
    contested: usize,
}

impl Issue {
    pub fn new(domain: Domain, cluster: Name, expiry: Instant) -> Self {
        Self {
            domain,
            cluster,
            expiry,
        }
    }

    pub fn domain(&self) -> &Domain {
        &self.domain
    }

    pub fn cluster(&self) -> &Name {
        &self.cluster
    }

    pub fn expiry(&self) -> Instant {
        self.expiry
    }
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, address: IpAddr, issue: Issue) -> bool {
        if spared(address) {
            return false;
        }
        let crossed = self
            .issued
            .get(&address)
            .is_some_and(|held| held.cluster() != issue.cluster());
        if crossed {
            self.contested += 1;
        }
        self.issued.insert(address, issue);
        true
    }

    pub fn contested(&self) -> usize {
        self.contested
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
