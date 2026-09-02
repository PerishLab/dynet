use crate::error::Error;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    half: Duration,
    floor: f64,
}

impl Policy {
    pub fn new(half: Duration, floor: f64) -> Result<Self, Error> {
        if half.is_zero() {
            return Err(Error::new(
                "a penalty that never decays retires the nodes reaching the hardest destinations",
            ));
        }
        if !(floor.is_finite() && floor > 0.0 && floor < 1.0) {
            return Err(Error::new(format!(
                "an exploration floor must lie between zero and one, not {floor}"
            )));
        }
        Ok(Self { half, floor })
    }

    pub fn half(self) -> Duration {
        self.half
    }

    pub fn floor(self) -> f64 {
        self.floor
    }
}
