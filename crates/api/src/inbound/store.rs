use crate::resolver::Chain;
use dynet_core::Name;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EARLY: f64 = 0.8;
pub const PATIENCE: usize = 2;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Key {
    pub stamp: String,
    pub name: String,
    pub cluster: Name,
}

#[derive(Clone, Debug)]
struct Kept {
    chain: Chain,
    born: Instant,
    life: Duration,
    busy: bool,
    misses: usize,
}

pub enum Recall {
    Known {
        chain: Chain,
        aged: u32,
        refresh: bool,
    },
    Absent,
}

#[derive(Debug, Default)]
pub struct Store {
    held: Mutex<HashMap<Key, Kept>>,
}

impl Store {
    pub fn clear(&self) {
        if let Ok(mut held) = self.held.lock() {
            held.clear();
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn recall(&self, key: &Key) -> Recall {
        let Ok(mut held) = self.held.lock() else {
            return Recall::Absent;
        };
        let Some(kept) = held.get_mut(key) else {
            return Recall::Absent;
        };
        let aged = kept.born.elapsed();
        if !kept.chain.links.is_empty() && aged >= kept.life {
            held.remove(key);
            return Recall::Absent;
        }
        let ripe = aged.as_secs_f64() >= kept.life.as_secs_f64() * EARLY;
        let refresh = ripe && !kept.busy;
        kept.busy |= refresh;
        Recall::Known {
            chain: kept.chain.clone(),
            aged: u32::try_from(aged.as_secs()).unwrap_or(u32::MAX),
            refresh,
        }
    }

    pub fn keep(&self, key: Key, chain: Chain, life: Duration) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        held.insert(
            key,
            Kept {
                chain,
                born: Instant::now(),
                life,
                busy: false,
                misses: 0,
            },
        );
    }

    pub fn missed(&self, key: &Key) -> usize {
        let Ok(mut held) = self.held.lock() else {
            return PATIENCE;
        };
        let Some(kept) = held.get_mut(key) else {
            return PATIENCE;
        };
        kept.busy = false;
        kept.misses += 1;
        let spent = kept.misses;
        if spent >= PATIENCE {
            held.remove(key);
        }
        spent
    }
}
