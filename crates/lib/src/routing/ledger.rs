use super::domain::Domain;
use super::reserved::spared;
use crate::Name;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

pub const NAMES: usize = 32;
pub const CUSHION: u64 = 120;
pub const FLOOR: u64 = 86_400;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Issue {
    names: Vec<Domain>,
    cluster: Name,
    issued: u64,
    expires: u64,
    retained: u64,
    #[serde(default)]
    crowded: bool,
    #[serde(default)]
    released: bool,
    #[serde(skip)]
    sessions: u32,
}

#[derive(Deserialize, Serialize)]
struct Record {
    address: IpAddr,
    #[serde(flatten)]
    issue: Issue,
}

#[derive(Clone, Debug)]
pub struct Ledger {
    issued: BTreeMap<IpAddr, Issue>,
    contested: usize,
    floor: u64,
}

impl Issue {
    pub fn names(&self) -> &[Domain] {
        &self.names
    }

    pub fn domain(&self) -> &Domain {
        &self.names[self.names.len() - 1]
    }

    pub fn cluster(&self) -> &Name {
        &self.cluster
    }

    pub fn issued(&self) -> u64 {
        self.issued
    }

    pub fn expires(&self) -> u64 {
        self.expires
    }

    pub fn retained(&self) -> u64 {
        self.retained
    }

    pub fn crowded(&self) -> bool {
        self.crowded
    }

    pub fn released(&self) -> bool {
        self.released
    }

    pub fn sessions(&self) -> u32 {
        self.sessions
    }

    pub(crate) fn moved(&mut self, cluster: Name) {
        self.cluster = cluster;
    }

    pub(crate) fn release(&mut self) {
        self.released = true;
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new(FLOOR)
    }
}

impl Ledger {
    pub fn new(floor: u64) -> Self {
        Self {
            issued: BTreeMap::new(),
            contested: 0,
            floor,
        }
    }

    pub fn read(text: &str, floor: u64) -> Result<Self, Error> {
        let records: Vec<Record> = serde_json::from_str(text)
            .map_err(|error| Error::new(format!("the ledger does not parse: {error}")))?;
        let mut ledger = Self::new(floor);
        for record in records {
            if record.issue.names.is_empty() {
                return Err(Error::new(format!(
                    "the ledger holds {} under no name",
                    record.address
                )));
            }
            if !spared(record.address) {
                ledger.issued.insert(record.address, record.issue);
            }
        }
        Ok(ledger)
    }

    pub fn write(&self) -> String {
        let records: Vec<Record> = self
            .issued
            .iter()
            .map(|(address, issue)| Record {
                address: *address,
                issue: issue.clone(),
            })
            .collect();
        serde_json::to_string(&records).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn record(&mut self, address: IpAddr, told: (&Domain, &Name), life: (u64, u64)) -> bool {
        if spared(address) {
            return false;
        }
        let (domain, cluster) = told;
        let (ttl, now) = life;
        let expires = now.saturating_add(ttl);
        let retained = expires
            .saturating_add(CUSHION)
            .max(now.saturating_add(self.floor));
        let issue = self.issued.entry(address).or_insert_with(|| Issue {
            names: Vec::new(),
            cluster: cluster.clone(),
            issued: now,
            expires,
            retained,
            crowded: false,
            released: false,
            sessions: 0,
        });
        if &issue.cluster != cluster {
            self.contested += 1;
            issue.cluster = cluster.clone();
        }
        issue.names.retain(|name| name != domain);
        issue.names.push(domain.clone());
        if issue.names.len() > NAMES {
            issue.names.remove(0);
            issue.crowded = true;
        }
        issue.released = false;
        issue.issued = now;
        issue.expires = issue.expires.max(expires);
        issue.retained = issue.retained.max(retained);
        true
    }

    pub fn contested(&self) -> usize {
        self.contested
    }

    pub fn lookup(&self, address: IpAddr) -> Option<&Issue> {
        self.issued.get(&address)
    }

    pub fn addresses(&self) -> impl Iterator<Item = IpAddr> + '_ {
        self.issued.keys().copied()
    }

    pub fn entries(&self) -> impl Iterator<Item = (IpAddr, &Issue)> + '_ {
        self.issued.iter().map(|(address, issue)| (*address, issue))
    }

    pub fn engage(&mut self, standing: &BTreeMap<IpAddr, u32>) {
        for (address, issue) in &mut self.issued {
            issue.sessions = standing.get(address).copied().unwrap_or_default();
        }
    }

    pub fn sweep(&mut self, now: u64) -> Vec<IpAddr> {
        let gone: Vec<IpAddr> = self
            .issued
            .iter()
            .filter(|(_, issue)| issue.sessions == 0)
            .filter(|(_, issue)| issue.released || issue.retained <= now)
            .map(|(address, _)| *address)
            .collect();
        for address in &gone {
            self.issued.remove(address);
        }
        gone
    }

    pub(crate) fn issue(&mut self, address: IpAddr) -> Option<&mut Issue> {
        self.issued.get_mut(&address)
    }
}
