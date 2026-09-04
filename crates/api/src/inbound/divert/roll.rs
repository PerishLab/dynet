use crate::inbound::IDLE;
use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::time::Instant;

const FIRST: u16 = 20000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Held {
    pub(crate) caller: SocketAddrV4,
    pub(crate) target: SocketAddrV4,
}

struct Kept {
    held: Held,
    ended: Option<Instant>,
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
                kept.ended = None;
            }
            return Some((spare, false));
        }
        let spare = self.take()?;
        self.kept.insert(spare, Kept { held, ended: None });
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

    pub(super) fn reap(&mut self) -> usize {
        let gone: Vec<u16> = self
            .kept
            .iter()
            .filter(|(_, kept)| kept.ended.is_some_and(|when| when.elapsed() >= IDLE))
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
