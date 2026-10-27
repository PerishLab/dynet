use super::wall::Wall;
use super::{paths, refused};
use crate::host::{Fragment, Held, held, run};
use dynet_core::{Error, Instance, Range, Span};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settled {
    pub added: usize,
    pub dropped: usize,
}

pub struct Route;
pub struct Link;

impl Link {
    pub fn present(instance: &Instance) -> bool {
        dynet_tun::index(&dynet_tun::name(instance.get())).is_ok()
    }

    pub fn dress(instance: &Instance, span: Span) -> Result<(), Error> {
        let path = paths::path(instance, "link");
        let seat = span.seat().to_string();
        match held(&path)? {
            Held::Foreign => {
                return Err(Error::new(
                    "the saved resolver route belongs to another owner",
                ));
            }
            Held::Owned => {
                let text = std::fs::read_to_string(&path).map_err(refused)?;
                if text.lines().nth(1) != Some(seat.as_str()) {
                    return Err(Error::new(
                        "stop the prior instance before changing its resolver seat",
                    ));
                }
            }
            Held::Absent => {
                if local(&seat) {
                    return Err(Error::new(
                        "the resolver seat already has a foreign local route",
                    ));
                }
                Fragment::new(&path, &seat).write()?;
            }
        }
        let name = dynet_tun::name(instance.get());
        let spare = span.spare().to_string();
        run(
            "/sbin/ifconfig",
            &[
                &name,
                "inet",
                &seat,
                &spare,
                "netmask",
                &mask(span.prefix()),
                "mtu",
                "1400",
                "up",
            ],
        )?;
        if !local(&seat) {
            run(
                "/sbin/route",
                &["-n", "add", "-inet", "-host", &seat, "-interface", "lo0"],
            )?;
        }
        if !local(&seat) {
            return Err(Error::new("the resolver seat has no local route"));
        }
        Ok(())
    }

    pub fn destroy(instance: &Instance) -> Result<bool, Error> {
        let path = paths::path(instance, "link");
        match held(&path)? {
            Held::Absent => return Ok(false),
            Held::Foreign => {
                return Err(Error::new(
                    "the saved resolver route belongs to another owner",
                ));
            }
            Held::Owned => (),
        }
        let text = std::fs::read_to_string(&path).map_err(refused)?;
        let seat = text
            .lines()
            .nth(1)
            .ok_or_else(|| Error::new("the saved resolver route has no seat"))?;
        seat.parse::<Ipv4Addr>()
            .map_err(|_| Error::new("the saved resolver seat is not IPv4"))?;
        if local(seat) {
            run(
                "/sbin/route",
                &["-n", "delete", "-inet", "-host", seat, "-interface", "lo0"],
            )?;
        }
        Fragment::remove(&path)
    }
}

fn local(seat: &str) -> bool {
    run("/sbin/route", &["-n", "get", "-inet", seat])
        .is_ok_and(|text| text.lines().any(|line| line.trim() == "interface: lo0"))
}

impl Route {
    pub fn hold(instance: &Instance, address: IpAddr) -> bool {
        if address.is_ipv6() || Wall(instance).hold(address).is_err() {
            return false;
        }
        let range = Range::host(address);
        Self::shown(instance)
            .and_then(|shown| {
                if !shown.contains(&range) {
                    change(instance, range, "add")?;
                }
                verify(instance, range)
            })
            .is_ok()
    }

    pub fn settle(instance: &Instance, wanted: &BTreeSet<Range>) -> Result<Settled, Error> {
        if wanted.iter().any(|range| range.base().is_ipv6()) {
            return Err(Error::new("IPv6 capture is not offered on macOS"));
        }
        let present = Self::shown(instance)?;
        Wall(instance).settle(wanted)?;
        let mut settled = Settled::default();
        for range in wanted.difference(&present) {
            change(instance, *range, "add")?;
            settled.added += 1;
        }
        for range in present.difference(wanted) {
            change(instance, *range, "delete")?;
            settled.dropped += 1;
        }
        let actual = Self::shown(instance)?;
        if !wanted.is_subset(&actual) {
            return Err(Error::new("the macOS route projection is incomplete"));
        }
        Ok(settled)
    }

    fn shown(instance: &Instance) -> Result<BTreeSet<Range>, Error> {
        let name = dynet_tun::name(instance.get());
        let text = run("/usr/sbin/netstat", &["-rn", "-f", "inet"])?;
        let mut found = BTreeSet::new();
        for line in text.lines() {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() < 4 || !parts.contains(&name.as_str()) {
                continue;
            }
            let flags = parts[2];
            if flags.contains('S') && !flags.contains('C') && !flags.contains('L') {
                found.insert(prefix(parts[0])?);
            }
        }
        Ok(found)
    }
}

fn change(instance: &Instance, range: Range, verb: &str) -> Result<(), Error> {
    let name = dynet_tun::name(instance.get());
    let prefix = format!("{}/{}", range.base(), range.prefix());
    run(
        "/sbin/route",
        &["-n", verb, "-inet", "-net", &prefix, "-interface", &name],
    )?;
    Ok(())
}

fn verify(instance: &Instance, range: Range) -> Result<(), Error> {
    if !Route::shown(instance)?.contains(&range) {
        return Err(Error::new("the issued route is absent after installation"));
    }
    Ok(())
}

fn prefix(text: &str) -> Result<Range, Error> {
    let (base, bits) = text.split_once('/').unwrap_or((text, "32"));
    let mut words: Vec<_> = base.split('.').collect();
    while words.len() < 4 {
        words.push("0");
    }
    Range::read(&format!("{}/{}", words.join("."), bits))
}

fn mask(prefix: u8) -> String {
    let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
    Ipv4Addr::from(mask).to_string()
}
