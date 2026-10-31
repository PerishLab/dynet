use std::sync::atomic::{AtomicUsize, Ordering};

pub(in crate::inbound) struct Budget {
    pub(super) active: AtomicUsize,
    pub(super) limit: usize,
}

pub(in crate::inbound) struct Permit<'a>(&'a Budget);

impl Budget {
    pub(in crate::inbound) fn new(limit: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            limit,
        }
    }

    pub(in crate::inbound) fn enter(&self) -> Option<Permit<'_>> {
        self.active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                (active < self.limit).then_some(active + 1)
            })
            .ok()
            .map(|_| Permit(self))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::Budget;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn concurrent() {
        let budget = Budget {
            active: AtomicUsize::new(0),
            limit: 3,
        };
        let barrier = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let barrier = &barrier;
                let budget = &budget;
                scope.spawn(move || {
                    let permit = budget.enter();
                    barrier.wait();
                    assert!(budget.enter().is_none());
                    barrier.wait();
                    drop(permit);
                });
            }
        });
        assert!(budget.enter().is_some());
    }
}
