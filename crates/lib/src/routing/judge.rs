use super::domain::Domain;
use super::ledger::{Issue, Ledger};
use super::range::Range;
use crate::{Manifest, Name};
use std::net::IpAddr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Sieve {
    Name(Domain),
    Address(IpAddr),
    Prefix(Range),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tally {
    pub moved: usize,
    pub released: usize,
    pub kept: usize,
    pub crowded: usize,
}

impl Sieve {
    pub fn read(told: &str) -> Option<Self> {
        if let Ok(address) = told.parse::<IpAddr>() {
            return Some(Self::Address(address));
        }
        if let Ok(range) = Range::read(told) {
            return Some(Self::Prefix(range));
        }
        Domain::new(told).ok().map(Self::Name)
    }

    pub fn passes(&self, address: IpAddr, issue: &Issue) -> bool {
        match self {
            Self::Name(parent) => issue.names().iter().any(|name| name.within(parent)),
            Self::Address(held) => *held == address,
            Self::Prefix(range) => range.holds(address),
        }
    }
}

impl Tally {
    pub fn touched(&self) -> bool {
        self.moved + self.released + self.kept + self.crowded > 0
    }
}

impl Ledger {
    pub fn flush(&mut self, manifest: &Manifest, sieve: &Sieve, dry: bool) -> Tally {
        let chosen: Vec<IpAddr> = self
            .entries()
            .filter(|(address, issue)| !issue.released() && sieve.passes(*address, issue))
            .map(|(address, _)| address)
            .collect();
        self.judge(manifest, &chosen, dry)
    }

    pub fn rejudge(&mut self, manifest: &Manifest) -> Tally {
        let chosen: Vec<IpAddr> = self
            .entries()
            .filter(|(_, issue)| !issue.released())
            .filter(|(_, issue)| manifest.cluster(issue.cluster()).is_none())
            .map(|(address, _)| address)
            .collect();
        self.judge(manifest, &chosen, false)
    }

    fn judge(&mut self, manifest: &Manifest, chosen: &[IpAddr], dry: bool) -> Tally {
        let mut tally = Tally::default();
        for address in chosen {
            let Some(issue) = self.issue(*address) else {
                continue;
            };
            let claimed = claimant(manifest, issue);
            ruled(issue, claimed, dry, &mut tally);
        }
        tally
    }
}

fn ruled(issue: &mut Issue, claimed: Option<Name>, dry: bool, tally: &mut Tally) {
    match claimed {
        Some(cluster) if &cluster == issue.cluster() => tally.kept += 1,
        Some(cluster) => {
            tally.moved += 1;
            if !dry {
                issue.moved(cluster);
            }
        }
        None if issue.crowded() => tally.crowded += 1,
        None => {
            tally.released += 1;
            if !dry {
                issue.release();
            }
        }
    }
}

fn claimant(manifest: &Manifest, issue: &Issue) -> Option<Name> {
    issue
        .names()
        .iter()
        .rev()
        .find_map(|name| manifest.table().claimed(name))
        .cloned()
}
