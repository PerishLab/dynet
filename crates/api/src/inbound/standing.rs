use super::warren::{Pools, Warren};
use crate::host::Fragment;
use dynet_core::{Bearing, Instance, Label, Name, Policy, Selector, Standing};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const ROOT: &str = "/run/dynet";

struct Reading {
    cluster: Name,
    bearing: Bearing,
    label: Label,
    standing: Standing,
}

pub fn path(instance: &Instance) -> PathBuf {
    PathBuf::from(format!("{ROOT}/{}.standing", instance.get()))
}

pub fn record(warren: &Warren, pools: &Pools, started: Instant) {
    let body = render(pools, started);
    if let Err(error) = Fragment::new(path(warren.instance), body).write() {
        (warren.told)(&format!("cannot record the standing: {error}"));
    }
}

fn render(pools: &Pools, started: Instant) -> String {
    let now = Instant::now();
    let mut names: Vec<&Name> = pools.keys().collect();
    names.sort();
    let mut lines = vec![format!(
        "taken {} standing {}",
        epoch(),
        started.elapsed().as_secs()
    )];
    for name in names {
        lines.extend(spoken(pools, name, now));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn spoken(pools: &Pools, cluster: &Name, now: Instant) -> Vec<String> {
    let Some(selector) = pools.get(cluster) else {
        return Vec::new();
    };
    let Ok(held) = selector.lock() else {
        return Vec::new();
    };
    let policy = held.policy();
    surveyed(cluster, &held)
        .iter()
        .map(|reading| told(reading, policy, now))
        .collect()
}

fn surveyed(cluster: &Name, selector: &Selector) -> Vec<Reading> {
    let mut readings = Vec::new();
    for bearing in [Bearing::Stream, Bearing::Datagram] {
        readings.extend(
            selector
                .survey(bearing)
                .into_iter()
                .map(|(label, standing)| Reading {
                    cluster: cluster.clone(),
                    bearing,
                    label,
                    standing,
                }),
        );
    }
    readings
}

fn told(reading: &Reading, policy: Policy, now: Instant) -> String {
    let standing = reading.standing;
    format!(
        "{} {} {:.3} {} {} {}",
        reading.cluster.get(),
        borne(reading.bearing),
        standing.weight(now, policy),
        standing.answered(),
        standing.charged(),
        reading.label.get()
    )
}

fn borne(bearing: Bearing) -> &'static str {
    match bearing {
        Bearing::Stream => "stream",
        Bearing::Datagram => "datagram",
    }
}

fn epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
