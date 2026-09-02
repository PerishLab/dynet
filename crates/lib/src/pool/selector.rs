use super::policy::Policy;
use super::standing::Standing;
use super::verdict::Verdict;
use crate::cluster::Cluster;
use crate::label::Label;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Selector {
    policy: Policy,
    standings: BTreeMap<Label, Standing>,
    running: BTreeMap<Label, f64>,
}

impl Selector {
    pub fn new(cluster: &Cluster, policy: Policy, now: Instant) -> Self {
        let mut standings = BTreeMap::new();
        let mut running = BTreeMap::new();
        for node in cluster.nodes() {
            standings.insert(node.label().clone(), Standing::new(now));
            running.insert(node.label().clone(), 0.0);
        }
        Self {
            policy,
            standings,
            running,
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn weight(&self, label: &Label, now: Instant) -> Option<f64> {
        let standing = self.standings.get(label)?;
        Some(standing.weight(now, self.policy))
    }

    pub fn choose(&mut self, now: Instant) -> Option<Label> {
        let mut total = 0.0;
        let mut best: Option<(Label, f64)> = None;
        for (label, standing) in &self.standings {
            let weight = standing.weight(now, self.policy);
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

    pub fn observed(&mut self, label: &Label, verdict: Verdict, now: Instant) {
        let policy = self.policy;
        let Some(standing) = self.standings.get_mut(label) else {
            return;
        };
        match verdict.blame().is_some_and(super::verdict::Blame::charges) {
            true => standing.charge(now, policy),
            false => standing.credited(now, policy),
        }
    }
}
