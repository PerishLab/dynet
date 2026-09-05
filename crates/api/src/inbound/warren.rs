use super::store::Store;
use super::warden;
use crate::outbound::{Book, Passage, Roster};
use crate::subscription::Entry;
use dynet_core::{
    Bearing, Cluster, Error, Instance, Label, Name, Policy, Router, Selector, Verdict,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub type Pools = HashMap<Name, Mutex<Selector>>;

const HALF: Duration = Duration::from_secs(600);
const FLOOR: f64 = 0.05;

#[derive(Debug, Default)]
pub struct Served {
    pub accepted: usize,
    pub answered: usize,
    pub faulted: usize,
    pub named: usize,
    pub refused: usize,
}

#[derive(Clone, Debug)]
pub struct Charge {
    pub cluster: Name,
    pub label: Label,
    pub bearing: Bearing,
}

pub struct Chosen {
    pub charge: Charge,
    pub told: String,
    pub passage: Passage,
}

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub entries: &'a [Entry],
    pub clusters: &'a [Cluster],
    pub router: &'a Mutex<Router>,
    pub port: u16,
    pub upstream: &'a str,
    pub book: &'a Book,
    pub store: &'a Store,
    pub told: &'a (dyn Fn(&str) + Sync),
}

impl Warren<'_> {
    pub fn forget(&self) {
        let Ok(mut router) = self.router.lock() else {
            return;
        };
        let gone = router.forget(Instant::now());
        drop(router);
        for address in &gone {
            crate::host::Route::release(self.instance, &address.to_string());
        }
        if !gone.is_empty() {
            (self.told)(&format!("released {} expired routes", gone.len()));
        }
    }

    pub fn passage(&self, pools: &Pools, wanted: &Name, bearing: Bearing) -> Option<Chosen> {
        let cluster = self.clusters.iter().find(|item| item.name() == wanted)?;
        let label = chosen(pools, wanted, bearing)?;
        let roster = Roster::new(self.entries, self.instance.mark());
        let Some(front) = cluster.via() else {
            let endpoint = roster.posted(&label, self.book, self.upstream).ok()?;
            let told = label.get().to_string();
            let charge = Charge {
                cluster: wanted.clone(),
                label,
                bearing,
            };
            return Some(Chosen {
                charge,
                told,
                passage: Passage::Plain(endpoint),
            });
        };
        let leading = chosen(pools, front, bearing)?;
        let endpoint = roster.posted(&leading, self.book, self.upstream).ok()?;
        let exit = roster.exit(&label).ok()?;
        let told = format!("{} through {}", label.get(), leading.get());
        let charge = Charge {
            cluster: front.clone(),
            label: leading,
            bearing,
        };
        Some(Chosen {
            charge,
            told,
            passage: Passage::Veiled(endpoint, exit),
        })
    }

    pub fn bearing(&self, wanted: &Name) -> bool {
        self.clusters
            .iter()
            .find(|item| item.name() == wanted)
            .is_some_and(Cluster::datagrams)
    }
}

pub fn observed(pools: &Pools, charge: &Charge, verdict: Verdict) {
    let Some(selector) = pools.get(&charge.cluster) else {
        return;
    };
    let Ok(mut held) = selector.lock() else {
        return;
    };
    held.observed(&charge.label, charge.bearing, verdict, Instant::now());
}

fn chosen(pools: &Pools, wanted: &Name, bearing: Bearing) -> Option<Label> {
    pools
        .get(wanted)?
        .lock()
        .ok()?
        .choose(bearing, Instant::now())
}

pub fn gather(warren: &Warren, started: Instant) -> Result<Pools, Error> {
    let policy = Policy::new(HALF, FLOOR)?;
    let mut pools = Pools::new();
    for cluster in warren.clusters {
        let selector = Selector::new(cluster, policy, started);
        pools.insert(cluster.name().clone(), Mutex::new(selector));
    }
    Ok(pools)
}

pub fn stand(warren: &Warren, pools: &Pools, span: Duration) {
    let Err(error) = warden::attend(warren, pools, span) else {
        return;
    };
    (warren.told)(&format!("the resolver refused to start: {error}"));
}
