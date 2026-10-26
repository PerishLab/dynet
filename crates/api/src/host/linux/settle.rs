use super::super::{feed, run};
use super::route::Route;
use dynet_core::{Error, Instance, Range};
use std::collections::BTreeSet;

const ABSENT: &str = "FIB table does not exist";
const PROTO: &str = "static";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settled {
    pub added: usize,
    pub dropped: usize,
}

impl Route {
    pub fn settle(instance: &Instance, wanted: &BTreeSet<Range>) -> Result<Settled, Error> {
        if wanted.iter().any(|range| range.base().is_ipv6()) {
            return Err(Error::new("IPv6 capture is not offered on this host"));
        }
        let present = Self::shown(instance)?;
        let name = instance.get();
        let added: Vec<&Range> = wanted.difference(&present).collect();
        let dropped: Vec<&Range> = present.difference(wanted).collect();
        let mut lines = String::new();
        for range in &added {
            lines.push_str(&format!(
                "route replace {} {}\n",
                spoken(**range),
                owned(name)
            ));
        }
        for range in &dropped {
            lines.push_str(&format!("route del {} {}\n", spoken(**range), owned(name)));
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

    pub fn shown(instance: &Instance) -> Result<BTreeSet<Range>, Error> {
        let name = instance.get();
        let shown = ["-4", "route", "show", "table", name, "proto", PROTO];
        let text = match run("ip", &shown) {
            Ok(text) => text,
            Err(error) if error.to_string().contains(ABSENT) => String::new(),
            Err(error) => return Err(error),
        };
        let device = format!(" dev {name} ");
        Ok(text
            .lines()
            .filter(|line| format!("{line} ").contains(&device))
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(read)
            .collect())
    }
}

pub(super) fn owned(name: &str) -> String {
    format!("dev {name} table {name} proto {PROTO}")
}

fn spoken(range: Range) -> String {
    format!("{}/{}", range.base(), range.prefix())
}

fn read(head: &str) -> Option<Range> {
    match head.contains('/') {
        true => Range::read(head).ok(),
        false => head.parse().ok().map(Range::host),
    }
}
