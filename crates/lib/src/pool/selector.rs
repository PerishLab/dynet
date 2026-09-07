use super::policy::Policy;
use super::standing::Standing;
use super::verdict::Verdict;
use crate::capability::Bearing;
use crate::cluster::Cluster;
use crate::label::Label;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Debug)]
struct Rank {
    standings: BTreeMap<Label, Standing>,
    running: BTreeMap<Label, f64>,
}

#[derive(Clone, Debug)]
pub struct Selector {
    policy: Policy,
    ranks: BTreeMap<Bearing, Rank>,
}

impl Rank {
    fn new(cluster: &Cluster, bearing: Bearing, now: Instant) -> Self {
        let mut standings = BTreeMap::new();
        let mut running = BTreeMap::new();
        for node in cluster.bearers(bearing) {
            standings.insert(node.label().clone(), Standing::new(now));
            running.insert(node.label().clone(), 0.0);
        }
        Self { standings, running }
    }

    fn choose(&mut self, policy: Policy, now: Instant) -> Option<Label> {
        let mut total = 0.0;
        let mut best: Option<(Label, f64)> = None;
        for (label, standing) in &self.standings {
            let weight = standing.weight(now, policy);
            total += weight;
            let current = self.running.entry(label.clone()).or_insert(0.0);
            *current += weight;
            let held = *current;
            if best.as_ref().is_none_or(|(_, top)| held > *top) {
                best = Some((label.clone(), held));
            }
        }
        let (label, _) = best?;
        if let Some(current) = self.running.get_mut(&label) {
            *current -= total;
        }
        Some(label)
    }

    fn observed(&mut self, label: &Label, verdict: Verdict, policy: Policy, now: Instant) {
        let Some(standing) = self.standings.get_mut(label) else {
            return;
        };
        match verdict.blame().is_some_and(super::verdict::Blame::charges) {
            true => standing.charge(now, policy),
            false => standing.credited(now, policy),
        }
    }
}

impl Selector {
    pub fn new(cluster: &Cluster, policy: Policy, now: Instant) -> Self {
        let mut ranks = BTreeMap::new();
        for bearing in [Bearing::Stream, Bearing::Datagram] {
            ranks.insert(bearing, Rank::new(cluster, bearing, now));
        }
        Self { policy, ranks }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn weight(&self, label: &Label, bearing: Bearing, now: Instant) -> Option<f64> {
        let standing = self.ranks.get(&bearing)?.standings.get(label)?;
        Some(standing.weight(now, self.policy))
    }

    pub fn survey(&self, bearing: Bearing) -> Vec<(Label, Standing)> {
        let Some(rank) = self.ranks.get(&bearing) else {
            return Vec::new();
        };
        rank.standings
            .iter()
            .map(|(label, standing)| (label.clone(), *standing))
            .collect()
    }

    pub fn choose(&mut self, bearing: Bearing, now: Instant) -> Option<Label> {
        let policy = self.policy;
        self.ranks.get_mut(&bearing)?.choose(policy, now)
    }

    pub fn observed(&mut self, label: &Label, bearing: Bearing, verdict: Verdict, now: Instant) {
        let policy = self.policy;
        let Some(rank) = self.ranks.get_mut(&bearing) else {
            return;
        };
        rank.observed(label, verdict, policy, now);
    }
}
