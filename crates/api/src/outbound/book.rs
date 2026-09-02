use crate::resolver::{self, Answer};
use dynet_core::Error;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EARLY: f64 = 0.8;
const PATIENCE: usize = 2;
const BRIEF: Duration = Duration::from_secs(1);

type Known = Mutex<HashMap<String, Held>>;

#[derive(Debug)]
struct Held {
    address: String,
    born: Instant,
    life: Duration,
    busy: bool,
    misses: usize,
}

enum Recall {
    Known { address: String, refresh: bool },
    Absent,
}

#[derive(Debug, Default)]
pub struct Book {
    known: Arc<Known>,
    mark: u32,
}

impl Book {
    pub fn new(mark: u32) -> Self {
        Self {
            known: Arc::default(),
            mark,
        }
    }

    pub fn find(&self, host: &str, upstream: &str) -> Result<String, Error> {
        if host.parse::<std::net::IpAddr>().is_ok() {
            return Ok(host.to_string());
        }
        let Recall::Known { address, refresh } = self.recall(host) else {
            return self.settle(host, upstream);
        };
        if refresh {
            self.again(host, upstream);
        }
        Ok(address)
    }

    fn recall(&self, host: &str) -> Recall {
        let Ok(mut known) = self.known.lock() else {
            return Recall::Absent;
        };
        let Some(held) = known.get_mut(host) else {
            return Recall::Absent;
        };
        let aged = held.born.elapsed().as_secs_f64();
        let ripe = aged >= held.life.as_secs_f64() * EARLY;
        let refresh = ripe && !held.busy;
        held.busy |= refresh;
        Recall::Known {
            address: held.address.clone(),
            refresh,
        }
    }

    fn settle(&self, host: &str, upstream: &str) -> Result<String, Error> {
        let answer = resolver::locate(host, upstream, self.mark)?;
        renew(&self.known, host, &answer);
        Ok(answer.address.to_string())
    }

    fn again(&self, host: &str, upstream: &str) {
        let known = Arc::clone(&self.known);
        let host = host.to_string();
        let upstream = upstream.to_string();
        let mark = self.mark;
        std::thread::spawn(move || match resolver::locate(&host, &upstream, mark) {
            Ok(answer) => renew(&known, &host, &answer),
            Err(_) => missed(&known, &host),
        });
    }
}

fn renew(known: &Known, host: &str, answer: &Answer) {
    let Ok(mut known) = known.lock() else {
        return;
    };
    known.insert(
        host.to_string(),
        Held {
            address: answer.address.to_string(),
            born: Instant::now(),
            life: Duration::from_secs(u64::from(answer.life)).max(BRIEF),
            busy: false,
            misses: 0,
        },
    );
}

fn missed(known: &Known, host: &str) {
    let Ok(mut known) = known.lock() else {
        return;
    };
    let Some(held) = known.get_mut(host) else {
        return;
    };
    held.busy = false;
    held.misses += 1;
    if held.misses >= PATIENCE {
        known.remove(host);
    }
}
