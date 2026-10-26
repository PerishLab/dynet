use super::domain::Domain;
use super::ledger::Ledger;
use crate::{Bearing, Form, Manifest, Name};
use std::net::IpAddr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Told {
    Own(Domain),
    Outer(Domain),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sight {
    pub told: Option<Told>,
    pub address: IpAddr,
    pub bearing: Bearing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ground {
    Named,
    Held,
    Unclaimed,
    Issued,
    Fallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Missing,
    Disabled,
    Empty,
    Unborne,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Course {
    Through(Name),
    Direct,
    Refused(Name, Refusal),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    course: Course,
    ground: Ground,
    aim: Option<Domain>,
}

impl Told {
    pub fn read(text: &str, outer: bool) -> Option<Self> {
        if text.parse::<IpAddr>().is_ok() {
            return None;
        }
        let domain = Domain::new(text).ok()?;
        Some(match outer {
            true => Self::Outer(domain),
            false => Self::Own(domain),
        })
    }

    pub fn domain(&self) -> &Domain {
        match self {
            Self::Own(domain) | Self::Outer(domain) => domain,
        }
    }
}

impl Decision {
    pub fn course(&self) -> &Course {
        &self.course
    }

    pub fn ground(&self) -> Ground {
        self.ground
    }

    pub fn aim(&self) -> Option<&Domain> {
        self.aim.as_ref()
    }

    pub fn cluster(&self) -> Option<&Name> {
        match &self.course {
            Course::Through(name) | Course::Refused(name, _) => Some(name),
            Course::Direct => None,
        }
    }
}

pub fn decide(manifest: &Manifest, ledger: &Ledger, sight: &Sight) -> Decision {
    let table = manifest.table();
    let issued = ledger.lookup(sight.address);
    if let Some(told) = &sight.told
        && let Some(cluster) = table.claimed(told.domain())
    {
        let aim = issued
            .is_none_or(|issue| issue.released())
            .then(|| told.domain().clone());
        return reach(manifest, cluster, sight.bearing, (Ground::Named, aim));
    }
    if let Some(cluster) = table.held(sight.address) {
        return reach(manifest, cluster, sight.bearing, (Ground::Held, None));
    }
    if let Some(Told::Own(_)) = &sight.told {
        return direct(Ground::Unclaimed);
    }
    if let Some(issue) = issued {
        if issue.released() {
            return direct(Ground::Issued);
        }
        return reach(
            manifest,
            issue.cluster(),
            sight.bearing,
            (Ground::Issued, None),
        );
    }
    match table.fallback() {
        Some(cluster) => reach(manifest, cluster, sight.bearing, (Ground::Fallback, None)),
        None => direct(Ground::Fallback),
    }
}

pub fn usable(manifest: &Manifest, name: &Name, bearing: Bearing) -> Result<(), Refusal> {
    let cluster = manifest.cluster(name).ok_or(Refusal::Missing)?;
    if cluster.disabled() {
        return Err(Refusal::Disabled);
    }
    match cluster.form() {
        Form::Alias(target) => usable(manifest, target, bearing),
        Form::Via(front, exit) => {
            if manifest.drained(exit) {
                return Err(Refusal::Empty);
            }
            usable(manifest, front, Bearing::Stream)?;
            match bearing {
                Bearing::Stream => Ok(()),
                Bearing::Datagram => Err(Refusal::Unborne),
            }
        }
        Form::Pool(nodes) if nodes.is_empty() => Err(Refusal::Empty),
        Form::Pool(_) => match cluster.bearers(bearing).is_empty() {
            true => Err(Refusal::Unborne),
            false => Ok(()),
        },
    }
}

fn reach(
    manifest: &Manifest,
    cluster: &Name,
    bearing: Bearing,
    told: (Ground, Option<Domain>),
) -> Decision {
    let (ground, aim) = told;
    let course = match usable(manifest, cluster, bearing) {
        Ok(()) => Course::Through(cluster.clone()),
        Err(refusal) => Course::Refused(cluster.clone(), refusal),
    };
    Decision {
        course,
        ground,
        aim,
    }
}

fn direct(ground: Ground) -> Decision {
    Decision {
        course: Course::Direct,
        ground,
        aim: None,
    }
}
