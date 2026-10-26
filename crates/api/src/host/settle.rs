use super::route::Route;
use super::{feed, run};
use dynet_core::{Error, Instance};
use std::collections::BTreeSet;
use std::net::IpAddr;

const ABSENT: &str = "FIB table does not exist";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settled {
    pub added: usize,
    pub dropped: usize,
}

impl Route {
    pub fn settle(instance: &Instance, wanted: &BTreeSet<IpAddr>) -> Result<Settled, Error> {
        let present = Self::shown(instance)?;
        let name = instance.get();
        let added: Vec<&IpAddr> = wanted.difference(&present).collect();
        let dropped: Vec<&IpAddr> = present.difference(wanted).collect();
        let mut lines = String::new();
        for address in &added {
            lines.push_str(&format!(
                "route replace {address}/32 dev {name} table {name}\n"
            ));
        }
        for address in &dropped {
            lines.push_str(&format!("route del {address}/32 dev {name} table {name}\n"));
        }
        if !lines.is_empty() {
            feed("ip", &["-force", "-batch", "-"], &lines)?;
        }
        let held = Self::shown(instance)?;
        let short = wanted.difference(&held).count();
        if short > 0 {
            return Err(Error::new(format!(
                "the kernel holds {} of {} issued addresses in table {name}",
                wanted.len() - short,
                wanted.len()
            )));
        }
        Ok(Settled {
            added: added.len(),
            dropped: dropped.len(),
        })
    }

    pub fn shown(instance: &Instance) -> Result<BTreeSet<IpAddr>, Error> {
        let name = instance.get();
        let text = match run("ip", &["-4", "route", "show", "table", name]) {
            Ok(text) => text,
            Err(error) if error.to_string().contains(ABSENT) => String::new(),
            Err(error) => return Err(error),
        };
        let device = format!(" dev {name} ");
        Ok(text
            .lines()
            .filter(|line| format!("{line} ").contains(&device))
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|head| head.parse::<IpAddr>().ok())
            .collect())
    }
}
