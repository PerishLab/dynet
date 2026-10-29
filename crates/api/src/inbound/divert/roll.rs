use crate::inbound::IDLE;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddrV4};
use std::time::Instant;

const FIRST: u16 = 20000;
const CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Held {
    pub(crate) caller: SocketAddrV4,
    pub(crate) target: SocketAddrV4,
}

struct Kept {
    held: Held,
    ended: Option<Instant>,
    touched: Instant,
    started: bool,
}

pub(super) struct Roll {
    kept: HashMap<u16, Kept>,
    spare: HashMap<Held, u16>,
    free: Vec<u16>,
    next: u32,
}

impl Roll {
    pub(super) fn new() -> Self {
        Self {
            kept: HashMap::new(),
            spare: HashMap::new(),
            free: Vec::new(),
            next: u32::from(FIRST),
        }
    }

    pub(super) fn claim(&mut self, held: Held) -> Option<(u16, bool)> {
        if let Some(spare) = self.spare.get(&held).copied() {
            if let Some(kept) = self.kept.get_mut(&spare) {
                kept.touched = Instant::now();
            }
            return Some((spare, false));
        }
        if self.kept.len() >= CAPACITY {
            return None;
        }
        let spare = self.take()?;
        self.kept.insert(
            spare,
            Kept {
                held,
                ended: None,
                touched: Instant::now(),
                started: false,
            },
        );
        self.spare.insert(held, spare);
        Some((spare, true))
    }

    pub(super) fn recall(&self, spare: u16) -> Option<Held> {
        self.kept.get(&spare).map(|kept| kept.held)
    }

    pub(super) fn end(&mut self, spare: u16) {
        let Some(kept) = self.kept.get_mut(&spare) else {
            return;
        };
        if kept.ended.is_none() {
            kept.ended = Some(Instant::now());
        }
    }

    pub(super) fn begin(&mut self, spare: u16) {
        if let Some(kept) = self.kept.get_mut(&spare) {
            kept.started = true;
            kept.ended = None;
        }
    }

    pub(super) fn reap(&mut self) -> usize {
        let gone: Vec<u16> = self
            .kept
            .iter()
            .filter(|(_, kept)| {
                kept.ended.is_some_and(|when| when.elapsed() >= IDLE)
                    || (!kept.started && kept.touched.elapsed() >= IDLE)
            })
            .map(|(spare, _)| *spare)
            .collect();
        for spare in &gone {
            if let Some(kept) = self.kept.remove(spare) {
                self.spare.remove(&kept.held);
            }
            self.free.push(*spare);
        }
        gone.len()
    }

    pub(super) fn engaged(&self) -> BTreeMap<IpAddr, u32> {
        let mut engaged = BTreeMap::new();
        for kept in self.kept.values().filter(|kept| kept.ended.is_none()) {
            *engaged
                .entry(IpAddr::V4(*kept.held.target.ip()))
                .or_default() += 1;
        }
        engaged
    }

    pub(super) fn standing(&self) -> usize {
        self.kept.len()
    }

    fn take(&mut self) -> Option<u16> {
        if let Some(spare) = self.free.pop() {
            return Some(spare);
        }
        let spare = u16::try_from(self.next).ok()?;
        self.next += 1;
        Some(spare)
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, Held, Roll};
    use crate::inbound::IDLE;
    use std::net::SocketAddrV4;
    use std::time::Instant;

    fn held(port: u16) -> Held {
        Held {
            caller: SocketAddrV4::new([127, 0, 0, 1].into(), port),
            target: SocketAddrV4::new([1, 1, 1, 1].into(), 443),
        }
    }

    #[test]
    fn abandoned_handshakes_are_reclaimed_but_live_sessions_survive() {
        let mut roll = Roll::new();
        let (abandoned, _) = roll.claim(held(1)).unwrap();
        let (live, _) = roll.claim(held(2)).unwrap();
        roll.begin(live);
        for kept in roll.kept.values_mut() {
            kept.touched = Instant::now() - IDLE;
        }
        assert_eq!(roll.reap(), 1);
        assert!(roll.recall(abandoned).is_none());
        assert!(roll.recall(live).is_some());
    }

    #[test]
    fn retransmission_does_not_revive_an_ended_session() {
        let mut roll = Roll::new();
        let (port, _) = roll.claim(held(1)).unwrap();
        roll.begin(port);
        roll.end(port);
        roll.kept.get_mut(&port).unwrap().ended = Some(Instant::now() - IDLE);
        roll.claim(held(1)).unwrap();
        assert_eq!(roll.reap(), 1);
        assert_eq!(roll.standing(), 0);
    }

    #[test]
    fn mapping_capacity_refuses_new_flows_without_breaking_existing_ones() {
        let mut roll = Roll::new();
        for port in 0..CAPACITY as u16 {
            roll.claim(held(port)).unwrap();
        }
        assert!(roll.claim(held(CAPACITY as u16)).is_none());
        assert!(roll.claim(held(0)).is_some());
    }
}
