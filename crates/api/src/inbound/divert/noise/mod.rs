mod frame;
#[cfg(test)]
mod tests;

use frame::Frame;
pub(super) use frame::Key;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub(super) struct Noise {
    frame: Mutex<Frame>,
    failed: AtomicBool,
}

impl Noise {
    pub(super) fn new() -> Self {
        Self {
            frame: Mutex::new(Frame::new(Instant::now())),
            failed: AtomicBool::new(false),
        }
    }

    pub(super) fn refuse(
        &self,
        key: Key,
        line: impl FnOnce() -> String,
        told: &(dyn Fn(&str) + Sync),
    ) {
        let result = self.frame.lock().map(|mut frame| {
            let now = Instant::now();
            let summaries = frame.rotate(now, false);
            let (first, overflow) = frame.record(&key, now);
            Pending {
                lines: summaries,
                first,
                overflow,
                window: frame.number,
            }
        });
        match result {
            Ok(pending) => pending.tell(&key, line, told),
            Err(error) => {
                drop(error);
                self.failed(told);
            }
        }
    }

    pub(super) fn flush(&self, finish: bool, told: &(dyn Fn(&str) + Sync)) {
        let result = self
            .frame
            .lock()
            .map(|mut frame| frame.rotate(Instant::now(), finish));
        match result {
            Ok(summaries) => emit(summaries, told),
            Err(error) => {
                drop(error);
                self.failed(told);
            }
        }
    }

    fn failed(&self, told: &(dyn Fn(&str) + Sync)) {
        if !self.failed.swap(true, Ordering::Relaxed) {
            told(
                "UDP refusal statistics unavailable; counts are unknown, packet handling is unchanged",
            );
        }
    }
}

fn emit(lines: Vec<String>, told: &(dyn Fn(&str) + Sync)) {
    for line in lines {
        told(&line);
    }
}

struct Pending {
    lines: Vec<String>,
    first: bool,
    overflow: bool,
    window: u64,
}

impl Pending {
    fn tell(self, key: &Key, line: impl FnOnce() -> String, told: &(dyn Fn(&str) + Sync)) {
        emit(self.lines, told);
        if self.first {
            let scope = if self.overflow { "overflow" } else { "bucket" };
            told(&format!(
                "udp refusal first window={} scope={scope} {}: {}",
                self.window,
                key.spoken(),
                line()
            ));
        }
    }
}
