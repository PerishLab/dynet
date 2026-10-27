use dynet_core::{Error, Instance, Range, Span};
use dynet_host::windows::{Interface, Prefix, Wall};
use std::collections::BTreeSet;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settled {
    pub added: usize,
    pub dropped: usize,
}

pub struct Route;

pub struct Link;

impl Route {
    pub fn hold(instance: &Instance, address: IpAddr) -> bool {
        let IpAddr::V4(address) = address else {
            return false;
        };
        let Ok(face) = Interface::named(instance.get()) else {
            return false;
        };
        Wall::new(instance.get()).hold((address, 32)).is_ok() && face.admit((address, 32)).is_ok()
    }

    pub fn settle(instance: &Instance, wanted: &BTreeSet<Range>) -> Result<Settled, Error> {
        if wanted.iter().any(|range| range.base().is_ipv6()) {
            return Err(Error::new("IPv6 capture is not offered on this host"));
        }
        let name = instance.get();
        let face = Interface::named(name)
            .map_err(|error| Error::new(format!("the device {name} cannot be found: {error}")))?;
        let wanted: BTreeSet<Prefix> = wanted.iter().filter_map(|range| prefix(*range)).collect();
        let wall = Wall::new(name);
        wall.settle(face.luid(), &wanted)
            .map_err(|error| Error::new(format!("the wall did not settle: {error}")))?;
        let present = face.routes().map_err(refused)?;
        let mut settled = Settled::default();
        for prefix in wanted.difference(&present) {
            face.admit(*prefix).map_err(refused)?;
            settled.added += 1;
        }
        for prefix in present.difference(&wanted) {
            face.evict(*prefix).map_err(refused)?;
            settled.dropped += 1;
        }
        let held = face.routes().map_err(refused)?;
        let short = wanted.difference(&held).count();
        if short > 0 {
            return Err(Error::new(format!(
                "the host holds {} of {} issued routes on {name}",
                wanted.len() - short,
                wanted.len()
            )));
        }
        if wall.holds().map_err(refused)? != wanted {
            return Err(Error::new("the wall differs from the requested capture"));
        }
        Ok(settled)
    }
}

impl Link {
    pub fn present(instance: &Instance) -> bool {
        Interface::named(instance.get()).is_ok()
    }

    pub fn dress(instance: &Instance, span: Span) -> Result<(), Error> {
        let face = Interface::named(instance.get()).map_err(refused)?;
        face.dress(span.seat(), span.prefix()).map_err(refused)
    }
}

fn prefix(range: Range) -> Option<Prefix> {
    match range.masked().base() {
        IpAddr::V4(base) => Some((base, range.prefix())),
        IpAddr::V6(_) => None,
    }
}

fn refused(error: std::io::Error) -> Error {
    Error::new(format!("the host refused: {error}"))
}
