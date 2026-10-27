use super::Veil;
use crate::host::{Cleared, Ground, Standing};
use dynet_core::{Error, Instance};

pub const PORT: u16 = 53;

pub fn establish(instance: &Instance, ground: &Ground) -> Result<Standing, Error> {
    if ground.port != PORT {
        return Err(Error::new(format!(
            "Windows sends names to the seat through NRPT, which reaches port {PORT} alone, not {}",
            ground.port
        )));
    }
    if !ground.sources.is_empty() || ground.transit != 0 {
        return Err(Error::new(
            "capturing what other machines forward is not offered on Windows",
        ));
    }
    if ground.claim != "0.0.0.0/0" {
        return Err(Error::new(
            "Windows captures the manifest's issued and held addresses; --claim must remain 0.0.0.0/0",
        ));
    }
    let cleared = Cleared::default();
    if ground.bare {
        return Err(Error::new(
            "Windows capture requires NRPT and WFP; --bare is not offered",
        ));
    }
    match Veil::raise(instance, ground.span.seat()) {
        Ok(()) => Ok(Standing {
            cleared,
            veiled: true,
        }),
        Err(error) => Err(error),
    }
}

pub fn reclaim(instance: &Instance) -> Result<Cleared, Error> {
    Ok(Cleared {
        veil: Veil::lower(instance)?,
        route: false,
        link: false,
    })
}
