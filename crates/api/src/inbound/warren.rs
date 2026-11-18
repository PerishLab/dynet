use super::store::Store;
use super::warden;
use crate::control::{Control, Payload, Projection};
use crate::host::Journal;
use crate::manifest::Loaded;
use crate::outbound::{Book, Circuit, Fence, Passage, Roster};
use dynet_core::{
    Bearing, Cluster, Decision, Domain, Error, Form, Instance, Label, Ledger, Manifest, Name,
    Policy, Selector, Sight, Verdict, decide, usable,
};
use std::collections::HashMap;
use std::net::{TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

type Rank = Arc<Mutex<Selector>>;
pub type Pools = HashMap<Name, Rank>;

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
    rank: Rank,
    label: Label,
    bearing: Bearing,
}

pub struct Chosen {
    pub charge: Option<Charge>,
    pub told: String,
    pub passage: Passage,
}

pub struct Selection {
    held: Arc<Held>,
    circuit: Arc<Circuit>,
}

pub struct Held {
    pub loaded: Loaded,
    pub pools: Pools,
    pub upstream: String,
    pub retain: u64,
    pub paused: bool,
    pub stamp: String,
    pub fence: Arc<Fence>,
}

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub initial: &'a secrets::Assignment,
    pub held: &'a RwLock<Arc<Held>>,
    pub ledger: &'a Mutex<Ledger>,
    pub journal: &'a Journal,
    pub control: &'a Control,
    pub projection: &'a dyn Projection,
    pub reload: &'a AtomicBool,
    pub halt: &'a AtomicBool,
    pub port: u16,
    pub book: &'a Book,
    pub store: &'a Store,
    pub told: &'a (dyn Fn(&str) + Sync),
}

impl Held {
    pub fn new(payload: Payload, earlier: Option<&Self>) -> Result<Self, Error> {
        let Payload {
            loaded,
            upstream,
            retain,
        } = payload;
        let policy = Policy::new(HALF, FLOOR)?;
        let now = Instant::now();
        let mut pools = Pools::new();
        let manifest = loaded.manifest();
        for cluster in manifest.clusters() {
            let rank = earlier
                .and_then(|held| held.kept(cluster, manifest))
                .or_else(|| {
                    ranked(manifest, cluster, policy, now)
                        .map(Mutex::new)
                        .map(Arc::new)
                });
            if let Some(rank) = rank {
                pools.insert(cluster.name().clone(), rank);
            }
        }
        let fence = earlier
            .filter(|held| !held.paused && !held.fence.closed())
            .map_or_else(|| Arc::new(Fence::new()), |held| Arc::clone(&held.fence));
        Ok(Self {
            loaded,
            pools,
            upstream: upstream.to_string(),
            retain,
            paused: false,
            stamp: String::new(),
            fence,
        })
    }

    fn kept(&self, cluster: &Cluster, manifest: &Manifest) -> Option<Rank> {
        let prior = self.loaded.manifest();
        let standing = prior.cluster(cluster.name())?;
        match cluster.form() {
            Form::Pool(_) if standing.nodes() == cluster.nodes() => {}
            Form::Via(front, exit) if standing == cluster => {
                usable(manifest, cluster.name(), Bearing::Datagram).ok()?;
                if prior.cluster(front)? != manifest.cluster(front)?
                    || prior.node(exit)? != manifest.node(exit)?
                {
                    return None;
                }
            }
            _ => return None,
        }
        self.pools.get(cluster.name()).cloned()
    }
}

fn ranked(
    manifest: &Manifest,
    cluster: &Cluster,
    policy: Policy,
    now: Instant,
) -> Option<Selector> {
    match cluster.form() {
        Form::Pool(_) => Some(Selector::new(cluster, policy, now)),
        Form::Via(front, _) => {
            usable(manifest, cluster.name(), Bearing::Datagram).ok()?;
            Some(Selector::relay(manifest.cluster(front)?, policy, now))
        }
        Form::Alias(_) => None,
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

    pub fn decide(&self, sight: &Sight) -> Option<(Decision, Selection)> {
        let ledger = self.ledger.lock().ok()?;
        let selection = self.selection()?;
        let decision = decide(selection.held.loaded.manifest(), &ledger, sight);
        Some((decision, selection))
    }

    pub fn inspect(&self, sight: &Sight) -> Option<Decision> {
        let ledger = self.ledger.lock().ok()?;
        let held = self.current();
        if held.paused || held.fence.closed() {
            return None;
        }
        Some(decide(held.loaded.manifest(), &ledger, sight))
    }

    fn selection(&self) -> Option<Selection> {
        let held = self.current();
        if held.paused {
            return None;
        }
        let circuit = held.fence.ticket().ok()?;
        Some(Selection { held, circuit })
    }

    pub fn claimed(&self, domain: &Domain) -> Option<Name> {
        let held = self.current();
        held.loaded.manifest().table().claimed(domain).cloned()
    }

    pub fn passage(&self, wanted: &Name, bearing: Bearing) -> Option<Chosen> {
        self.pick(self.selection()?, wanted, bearing)
    }

    pub fn pick(&self, selection: Selection, wanted: &Name, bearing: Bearing) -> Option<Chosen> {
        let Selection { held, circuit } = selection;
        usable(held.loaded.manifest(), wanted, bearing).ok()?;
        let cluster = held.loaded.manifest().pool(wanted)?;
        let roster = Roster::new(held.loaded.entries(), self.instance.mark());
        let (front, exit) = match cluster.form() {
            Form::Pool(_) => (cluster.name(), None),
            Form::Via(front, exit) => (front, Some(exit)),
            Form::Alias(_) => return None,
        };
        let pool = match (exit, bearing) {
            (Some(_), Bearing::Datagram) => cluster.name(),
            _ => front,
        };
        let rank = held.pools.get(pool)?.clone();
        let label = rank.lock().ok()?.choose(bearing, Instant::now())?;
        let endpoint = roster.posted(&label, self.book, &held.upstream).ok()?;
        let charge = Some(Charge {
            rank,
            label: label.clone(),
            bearing,
        });
        let Some(exit) = exit else {
            return Some(Chosen {
                charge,
                told: label.get().to_string(),
                passage: Passage::Plain(endpoint).gated(circuit),
            });
        };
        Some(Chosen {
            charge,
            told: format!("{} through {}", exit.get(), label.get()),
            passage: Passage::Veiled(endpoint, roster.exit(exit).ok()?).gated(circuit),
        })
    }

    pub fn direct(&self, selection: Selection) -> Option<Chosen> {
        let circuit = selection.circuit;
        if circuit.closed() {
            return None;
        }
        Some(Chosen {
            charge: None,
            told: "direct".to_string(),
            passage: Passage::Direct(self.instance.mark()).gated(circuit),
        })
    }
}

pub fn observed(charge: Option<&Charge>, verdict: Verdict) {
    let Some(charge) = charge else {
        return;
    };
    let Ok(mut pool) = charge.rank.lock() else {
        return;
    };
    pool.observed(&charge.label, charge.bearing, verdict, Instant::now());
}

pub fn stand(warren: &Warren, span: Duration, socket: &(UdpSocket, TcpListener)) {
    let Err(error) = warden::attend(warren, span, socket) else {
        return;
    };
    (warren.told)(&format!("the resolver refused to start: {error}"));
}
