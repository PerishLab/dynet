use super::call::Call;
use std::sync::atomic::{AtomicUsize, Ordering};

const LIMIT: usize = 16;

pub(super) struct Admission {
    direct: AtomicUsize,
    proxy: AtomicUsize,
}

pub(super) struct Permit<'a>(&'a AtomicUsize);

impl Admission {
    pub fn new() -> Self {
        Self {
            direct: AtomicUsize::new(0),
            proxy: AtomicUsize::new(0),
        }
    }

    pub fn enter(&self, call: &Call) -> Option<Permit<'_>> {
        let used = match call.cluster {
            Some(_) => &self.proxy,
            None => &self.direct,
        };
        used.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |count| {
            (count < LIMIT).then_some(count + 1)
        })
        .ok()
        .map(|_| Permit(used))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
