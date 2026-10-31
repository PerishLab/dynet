use super::{Link, Veil};
use crate::host::{Cleared, Ground, Standing};
use dynet_core::{Error, Instance};

pub fn establish(instance: &Instance, ground: &Ground) -> Result<Standing, Error> {
    if !ground.local {
        return Err(Error::new(
            "forwarded-only capture is offered on Linux alone",
        ));
    }
    if ground.port != 53 || ground.claim != "0.0.0.0/0" || ground.bare {
        return Err(Error::new(
            "macOS capture requires port 53, the issued/held route scope and PF protection",
        ));
    }
    if !ground.sources.is_empty() || ground.transit != 0 {
        return Err(Error::new(
            "macOS captures this host alone; forwarded sources and transit refuse",
        ));
    }
    Veil::raise(instance, ground.span.seat())?;
    Ok(Standing {
        cleared: Cleared::default(),
        veiled: true,
    })
}

pub fn reclaim(instance: &Instance) -> Result<Cleared, Error> {
    Ok(Cleared {
        veil: Veil::lower(instance)?,
        route: false,
        link: Link::destroy(instance)?,
    })
}
