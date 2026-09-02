use super::{Link, Route, Veil};
use dynet_core::{Error, Instance};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cleared {
    pub veil: bool,
    pub route: bool,
    pub link: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stray {
    pub links: Vec<String>,
    pub veils: Vec<String>,
}

impl Cleared {
    pub fn any(self) -> bool {
        self.veil || self.route || self.link
    }
}

pub fn reclaim(instance: &Instance) -> Result<Cleared, Error> {
    let veil = Veil::lower(instance)?;
    let route = Route::destroy(instance)?;
    let link = Link::destroy(instance)?;
    Ok(Cleared { veil, route, link })
}

pub fn sweep(instance: &Instance) -> Stray {
    let held = instance.get();
    Stray {
        links: Link::strays()
            .into_iter()
            .filter(|name| name != held)
            .collect(),
        veils: Veil::strays()
            .into_iter()
            .filter(|name| name != held)
            .collect(),
    }
}
