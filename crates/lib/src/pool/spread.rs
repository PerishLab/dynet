use crate::error::Error;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Affinity {
    window: Duration,
}

impl Affinity {
    pub fn new(window: Duration) -> Result<Self, Error> {
        if window.is_zero() {
            return Err(Error::new("an affinity window must outlast an instant"));
        }
        Ok(Self { window })
    }

    pub fn read(told: &str) -> Result<Self, Error> {
        let split = told.len().saturating_sub(1);
        let (count, unit) = told.split_at(split);
        let count: u64 = count
            .parse()
            .map_err(|_| Error::new(format!("{told} is not a window like 30s, 10m or 1h")))?;
        let seconds = match unit {
            "s" => count,
            "m" => count.saturating_mul(60),
            "h" => count.saturating_mul(3600),
            _ => return Err(Error::new(format!("{told} carries no unit of s, m or h"))),
        };
        Self::new(Duration::from_secs(seconds))
    }

    pub fn window(self) -> Duration {
        self.window
    }
}
