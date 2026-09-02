use super::store::Store;
use crate::outbound::{Book, Passage, Roster};
use crate::subscription::Entry;
use dynet_core::{Cluster, Instance, Label, Name, Router, Selector};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub type Pools = HashMap<Name, Mutex<Selector>>;

pub struct Warren<'a> {
    pub instance: &'a Instance,
    pub entries: &'a [Entry],
    pub clusters: &'a [Cluster],
    pub router: &'a Mutex<Router>,
    pub ports: &'a [u16],
    pub port: u16,
    pub upstream: &'a str,
    pub book: &'a Book,
    pub store: &'a Store,
    pub told: &'a (dyn Fn(&str) + Sync),
}

impl Warren<'_> {
    pub fn passage(&self, pools: &Pools, wanted: &Name) -> Option<(String, Passage)> {
        let cluster = self.clusters.iter().find(|item| item.name() == wanted)?;
        let label = chosen(pools, wanted)?;
        let roster = Roster::new(self.entries);
        let Some(front) = cluster.via() else {
            let endpoint = roster.posted(&label, self.book, self.upstream).ok()?;
            return Some((label.get().to_string(), Passage::Plain(endpoint)));
        };
        let leading = chosen(pools, front)?;
        let endpoint = roster.posted(&leading, self.book, self.upstream).ok()?;
        let exit = roster.exit(&label).ok()?;
        let told = format!("{} through {}", label.get(), leading.get());
        Some((told, Passage::Veiled(endpoint, exit)))
    }

    pub fn bearing(&self, wanted: &Name) -> bool {
        self.clusters
            .iter()
            .find(|item| item.name() == wanted)
            .is_some_and(Cluster::datagrams)
    }
}

fn chosen(pools: &Pools, wanted: &Name) -> Option<Label> {
    pools.get(wanted)?.lock().ok()?.choose(Instant::now())
}
