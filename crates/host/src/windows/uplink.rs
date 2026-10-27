use super::route::Interface;
use std::io::{Error, Result};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_secs(5);

static KNOWN: Mutex<Option<(Instant, u32)>> = Mutex::new(None);

pub fn uplink() -> Result<u32> {
    let mut known = KNOWN
        .lock()
        .map_err(|_| Error::other("the uplink is poisoned"))?;
    if let Some((when, index)) = *known
        && when.elapsed() < FRESH
    {
        return Ok(index);
    }
    let face = Interface::uplink(0)?
        .ok_or_else(|| Error::other("no interface carries a default route"))?;
    let index = face.index()?;
    *known = Some((Instant::now(), index));
    Ok(index)
}
