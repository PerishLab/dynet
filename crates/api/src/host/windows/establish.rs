use super::Veil;
use dynet_core::{Error, Instance, Span};

pub const PORT: u16 = 53;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cleared {
    pub veil: bool,
    pub route: bool,
    pub link: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub cleared: Cleared,
    pub veiled: bool,
}

pub struct Ground<'a> {
    pub port: u16,
    pub claim: &'a str,
    pub span: Span,
    pub bare: bool,
    pub sources: &'a [String],
    pub transit: u16,
}

impl Cleared {
    pub fn any(self) -> bool {
        self.veil || self.route || self.link
    }
}

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
