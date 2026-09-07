use super::policy::Policy;
use std::time::Instant;

const PENALTY: f64 = 0.5;
const RECOVERY: f64 = 0.25;

#[derive(Clone, Copy, Debug)]
pub struct Standing {
    credit: f64,
    touched: Instant,
    answered: u32,
    charged: u32,
}

impl Standing {
    pub fn new(now: Instant) -> Self {
        Self {
            credit: 1.0,
            touched: now,
            answered: 0,
            charged: 0,
        }
    }

    pub fn answered(&self) -> u32 {
        self.answered
    }

    pub fn charged(&self) -> u32 {
        self.charged
    }

    pub fn credit(&self, now: Instant, policy: Policy) -> f64 {
        let lapse = now.saturating_duration_since(self.touched).as_secs_f64();
        let decay = 0.5f64.powf(lapse / policy.half().as_secs_f64());
        1.0 - (1.0 - self.credit) * decay
    }

    pub fn weight(&self, now: Instant, policy: Policy) -> f64 {
        policy.floor() + (1.0 - policy.floor()) * self.credit(now, policy)
    }

    pub fn charge(&mut self, now: Instant, policy: Policy) {
        self.credit = self.credit(now, policy) * PENALTY;
        self.touched = now;
        self.charged = self.charged.saturating_add(1);
    }

    pub fn credited(&mut self, now: Instant, policy: Policy) {
        let held = self.credit(now, policy);
        self.credit = held + (1.0 - held) * RECOVERY;
        self.touched = now;
        self.answered = self.answered.saturating_add(1);
    }
}
