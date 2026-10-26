use super::store::Store;
use super::warden;
use crate::host::Journal;
use crate::manifest::{self, Loaded};
use crate::outbound::{Book, Passage, Roster};
use dynet_core::{
    Bearing, Cluster, Decision, Domain, Error, Form, Instance, Label, Ledger, Name, Policy,
    Selector, Sight, Verdict, decide, usable,
};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
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
    pub dropped: usize,
}

#[derive(Clone, Debug)]
pub struct Charge {
    pub cluster: Name,
    pub label: Label,
    pub bearing: Bearing,
}

pub struct Chosen {
    pub charge: Option<Charge>,
    pub told: String,
    pub passage: Passage,
}

pub struct Held {
    pub loaded: Loaded,
    pub pools: Pools,
}

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub held: &'a RwLock<Arc<Held>>,
    pub ledger: &'a Mutex<Ledger>,
    pub journal: &'a Journal,
    pub source: &'a Path,
    pub reload: &'a AtomicBool,
    pub halt: &'a AtomicBool,
    pub port: u16,
    pub upstream: &'a str,
    pub book: &'a Book,
    pub store: &'a Store,
    pub told: &'a (dyn Fn(&str) + Sync),
}

impl Held {
    pub fn new(loaded: Loaded, earlier: Option<&Self>) -> Result<Self, Error> {
        let policy = Policy::new(HALF, FLOOR)?;
        let now = Instant::now();
        let mut pools = Pools::new();
        for cluster in loaded.manifest().clusters() {
            if !matches!(cluster.form(), Form::Pool(_)) {
                continue;
            }
            let kept = earlier.and_then(|held| held.kept(cluster));
            let selector = kept.unwrap_or_else(|| Selector::new(cluster, policy, now));
            pools.insert(cluster.name().clone(), Mutex::new(selector));
        }
        Ok(Self { loaded, pools })
    }

    fn kept(&self, cluster: &Cluster) -> Option<Selector> {
        let standing = self.loaded.manifest().cluster(cluster.name())?;
        if standing.nodes() != cluster.nodes() {
            return None;
        }
        self.pools
            .get(cluster.name())?
            .lock()
            .ok()
            .map(|held| held.clone())
    }
}

impl Warren<'_> {
    pub fn halted(&self) -> bool {
        self.halt.load(Ordering::Relaxed)
    }

    pub fn current(&self) -> Arc<Held> {
        match self.held.read() {
            Ok(held) => Arc::clone(&held),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    pub fn decide(&self, sight: &Sight) -> Option<Decision> {
        let held = self.current();
        let ledger = self.ledger.lock().ok()?;
        Some(decide(held.loaded.manifest(), &ledger, sight))
    }

    pub fn claimed(&self, domain: &Domain) -> Option<Name> {
        let held = self.current();
        held.loaded.manifest().table().claimed(domain).cloned()
    }

    pub fn passage(&self, wanted: &Name, bearing: Bearing) -> Option<Chosen> {
        let held = self.current();
        usable(held.loaded.manifest(), wanted, bearing).ok()?;
        let cluster = held.loaded.manifest().pool(wanted)?;
        let roster = Roster::new(held.loaded.entries(), self.instance.mark());
        let (front, exit) = match cluster.form() {
            Form::Pool(_) => (cluster.name(), None),
            Form::Via(front, exit) => (front, Some(exit)),
            Form::Alias(_) => return None,
        };
        let label = chosen(&held.pools, front, bearing)?;
        let endpoint = roster.posted(&label, self.book, self.upstream).ok()?;
        let charge = Some(Charge {
            cluster: front.clone(),
            label: label.clone(),
            bearing,
        });
        let Some(exit) = exit else {
            return Some(Chosen {
                charge,
                told: label.get().to_string(),
                passage: Passage::Plain(endpoint),
            });
        };
        Some(Chosen {
            charge,
            told: format!("{} through {}", exit.get(), label.get()),
            passage: Passage::Veiled(endpoint, roster.exit(exit).ok()?),
        })
    }

    pub fn direct(&self) -> Chosen {
        Chosen {
            charge: None,
            told: "direct".to_string(),
            passage: Passage::Direct(self.instance.mark()),
        }
    }

    pub fn renew(&self) {
        let loaded = match manifest::read(self.source) {
            Ok(loaded) => loaded,
            Err(error) => {
                (self.told)(&format!(
                    "manifest refused, the standing one is kept: {error}"
                ));
                return;
            }
        };
        let standing = self.current();
        let held = match Held::new(loaded, Some(&standing)) {
            Ok(held) => held,
            Err(error) => {
                (self.told)(&format!(
                    "manifest refused, the standing one is kept: {error}"
                ));
                return;
            }
        };
        let count = held.loaded.manifest().table().rules().len();
        match self.held.write() {
            Ok(mut slot) => *slot = Arc::new(held),
            Err(poisoned) => *poisoned.into_inner() = Arc::new(held),
        }
        (self.told)(&format!("manifest replaced, {count} rules standing"));
        self.rejudged();
    }
}

pub fn observed(warren: &Warren, charge: Option<&Charge>, verdict: Verdict) {
    let Some(charge) = charge else {
        return;
    };
    let held = warren.current();
    let Some(selector) = held.pools.get(&charge.cluster) else {
        return;
    };
    let Ok(mut pool) = selector.lock() else {
        return;
    };
    pool.observed(&charge.label, charge.bearing, verdict, Instant::now());
}

fn chosen(pools: &Pools, wanted: &Name, bearing: Bearing) -> Option<Label> {
    pools
        .get(wanted)?
        .lock()
        .ok()?
        .choose(bearing, Instant::now())
}

pub fn stand(warren: &Warren, span: Duration, socket: &UdpSocket) {
    let Err(error) = warden::attend(warren, span, socket) else {
        return;
    };
    (warren.told)(&format!("the resolver refused to start: {error}"));
}
