use dynet_core::Name;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(super) const LIMIT: usize = 128;
pub(super) const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::inbound::divert) enum Key {
    Multicast(u16),
    Unborne(Name, u16),
}

impl Key {
    fn class(&self) -> usize {
        match self {
            Self::Multicast(_) => 0,
            Self::Unborne(_, _) => 1,
        }
    }

    pub(super) fn spoken(&self) -> String {
        match self {
            Self::Multicast(port) => format!("class=multicast port={port}"),
            Self::Unborne(cluster, port) => {
                format!("class=unborne cluster={} port={port}", cluster.get())
            }
        }
    }
}

pub(super) struct Frame {
    pub(super) number: u64,
    started: Instant,
    buckets: HashMap<Key, Count>,
    overflow: [Option<Count>; 2],
}

struct Count {
    first: Instant,
    last: Instant,
    seen: u64,
    full: bool,
}

impl Frame {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            number: 0,
            started: now,
            buckets: HashMap::new(),
            overflow: [None, None],
        }
    }

    pub(super) fn record(&mut self, key: &Key, now: Instant) -> (bool, bool) {
        if let Some(count) = self.buckets.get_mut(key) {
            count.note(now);
            return (false, false);
        }
        if self.buckets.len() < LIMIT {
            self.buckets.insert(key.clone(), Count::new(now));
            return (true, false);
        }
        let slot = &mut self.overflow[key.class()];
        match slot {
            Some(count) => {
                count.note(now);
                (false, true)
            }
            None => {
                *slot = Some(Count::new(now));
                (true, true)
            }
        }
    }

    pub(super) fn rotate(&mut self, now: Instant, finish: bool) -> Vec<String> {
        if !finish && now.saturating_duration_since(self.started) < WINDOW {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for (key, count) in self.buckets.drain() {
            let label = format!("window={} scope=bucket {}", self.number, key.spoken());
            lines.extend(count.summary(&label, self.started, now));
        }
        for (index, slot) in self.overflow.iter_mut().enumerate() {
            let Some(count) = slot.take() else { continue };
            let class = if index == 0 { "multicast" } else { "unborne" };
            let label = format!("window={} scope=overflow class={class}", self.number);
            lines.extend(count.summary(&label, self.started, now));
        }
        self.number = self.number.saturating_add(1);
        self.started = now;
        lines
    }
}

impl Count {
    fn new(now: Instant) -> Self {
        Self {
            first: now,
            last: now,
            seen: 1,
            full: false,
        }
    }

    fn note(&mut self, now: Instant) {
        self.last = now;
        match self.seen.checked_add(1) {
            Some(next) => self.seen = next,
            None => self.full = true,
        }
    }

    fn summary(&self, label: &str, started: Instant, now: Instant) -> Option<String> {
        if self.seen < 2 {
            return None;
        }
        let relation = if self.full { ">=" } else { "=" };
        Some(format!(
            "udp refusals {label} seen{relation}{} first=1 suppressed{relation}{} first_ms={} last_ms={} window_ms={}",
            self.seen,
            self.seen - 1,
            self.first.saturating_duration_since(started).as_millis(),
            self.last.saturating_duration_since(started).as_millis(),
            now.saturating_duration_since(started).as_millis(),
        ))
    }
}
