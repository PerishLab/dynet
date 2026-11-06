use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) struct Progress {
    pub(super) half: Mutex<Option<Instant>>,
    pub(super) quiet: Duration,
}

impl Progress {
    pub(super) fn touch(&self) {
        if let Ok(mut held) = self.half.lock()
            && held.is_some()
        {
            *held = Some(Instant::now());
        }
    }

    pub(super) fn finish(&self) {
        if let Ok(mut held) = self.half.lock() {
            held.get_or_insert_with(Instant::now);
        }
    }

    pub(super) fn expired(&self) -> bool {
        self.half.lock().map_or(true, |held| {
            held.is_some_and(|when| when.elapsed() >= self.quiet)
        })
    }
}
