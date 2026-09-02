use dynet_core::Name;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EARLY: f64 = 0.8;
pub const PATIENCE: usize = 2;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Key {
    pub name: String,
    pub cluster: Name,
}

#[derive(Clone, Debug)]
struct Kept {
    reply: Vec<u8>,
    born: Instant,
    life: Duration,
    busy: bool,
    misses: usize,
}

pub enum Recall {
    Known {
        reply: Vec<u8>,
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
        let ripe = aged.as_secs_f64() >= kept.life.as_secs_f64() * EARLY;
        let refresh = ripe && !kept.busy;
        kept.busy |= refresh;
        Recall::Known {
            reply: kept.reply.clone(),
            aged: u32::try_from(aged.as_secs()).unwrap_or(u32::MAX),
            refresh,
        }
    }

    pub fn keep(&self, key: Key, reply: Vec<u8>, life: Duration) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        held.insert(
            key,
            Kept {
                reply,
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
