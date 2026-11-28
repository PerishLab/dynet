use super::spec::{CADENCE, HEALTH, Spec};
use super::yaml::{Document, Group, required};
use crate::manifest::{Entry, Loaded};
use dynet_core::{Cluster, Error, Form, Label, Manifest, Name};
use std::collections::BTreeSet;
use std::fmt::Write as _;

struct Builder<'a> {
    manifest: &'a Manifest,
    entries: &'a [Entry],
    slot: u64,
    seen: BTreeSet<String>,
    proxies: Document,
}

pub fn render(loaded: &Loaded, spec: Option<&Spec>, now: u64) -> Result<String, Error> {
    let manifest = loaded.manifest();
    let cadence = spec.map(Spec::cadence).unwrap_or(CADENCE);
    let health = spec.map(Spec::health).unwrap_or(HEALTH);
    let slot = now / cadence;

    let wanted = referenced(manifest);
    let mut builder = Builder {
        manifest,
        entries: loaded.entries(),
        slot,
        seen: BTreeSet::new(),
        proxies: Document(String::new()),
    };
    let mut groups = Vec::new();
    for cluster in manifest.clusters() {
        if !wanted.contains(cluster.name()) || cluster.disabled() {
            continue;
        }
        let members = builder.cluster(cluster)?;
        groups.push(Group {
            name: cluster.name().get().to_owned(),
            members,
        });
    }

    let mut doc = Document(String::with_capacity(8192));
    let _ = writeln!(doc.0, "# Dynet Mihomo projection (slot={slot})");
    doc.header();
    doc.0.push_str("proxies:\n");
    doc.0.push_str(&builder.proxies.0);
    doc.0.push_str("proxy-groups:\n");
    for group in &groups {
        doc.listing(group, health);
    }
    doc.rules(manifest);
    Ok(doc.0)
}

impl Builder<'_> {
    fn cluster(&mut self, cluster: &Cluster) -> Result<Vec<String>, Error> {
        match cluster.form() {
            Form::Via(pool, exit) => self.via(cluster.name(), pool, exit),
            Form::Pool(_) | Form::Alias(_) => self.direct(cluster.name()),
        }
    }

    fn via(&mut self, name: &Name, pool: &Name, exit: &Label) -> Result<Vec<String>, Error> {
        if self.manifest.drained(exit) {
            return Err(Error::new(format!(
                "exit {} for cluster {} is drained",
                exit.get(),
                name.get()
            )));
        }
        let fronts = members(self.manifest, pool)?;
        let target = lookup(self.entries, exit)?;
        let tail = exit
            .get()
            .split_once('/')
            .map_or(exit.get(), |(_, suffix)| suffix);
        let mut legs = Vec::with_capacity(fronts.len());
        for front in &fronts {
            self.front(lookup(self.entries, front)?)?;
            let head = front
                .get()
                .split_once('/')
                .map_or(front.get(), |(prefix, _)| prefix);
            let leg = format!("{}-{head}-{tail}", name.get());
            if self.seen.insert(leg.clone()) {
                self.proxies.chained(&leg, target, front.get())?;
            }
            legs.push(leg);
        }
        Ok(rotate(legs, self.slot))
    }

    fn direct(&mut self, name: &Name) -> Result<Vec<String>, Error> {
        let fronts = members(self.manifest, name)?;
        let mut names = Vec::with_capacity(fronts.len());
        for front in &fronts {
            self.front(lookup(self.entries, front)?)?;
            names.push(front.get().to_owned());
        }
        Ok(rotate(names, self.slot))
    }

    fn front(&mut self, entry: &Entry) -> Result<(), Error> {
        let name = required(entry, "name")?;
        if !self.seen.insert(name.to_owned()) {
            return Ok(());
        }
        let kind = required(entry, "type")?;
        let addr = (required(entry, "server")?, required(entry, "port")?);
        match kind {
            "vless" => self.proxies.vless(entry, name, addr),
            "vmess" => self.proxies.vmess(entry, name, addr),
            other => Err(Error::new(format!("unsupported front protocol {other}"))),
        }
    }
}

fn referenced(manifest: &Manifest) -> BTreeSet<Name> {
    let mut set = BTreeSet::new();
    for rule in manifest.table().rules() {
        set.insert(rule.cluster().clone());
    }
    if let Some(fallback) = manifest.table().fallback() {
        set.insert(fallback.clone());
    }
    set
}

fn members(manifest: &Manifest, name: &Name) -> Result<Vec<Label>, Error> {
    let pool = manifest
        .pool(name)
        .ok_or_else(|| Error::new(format!("cluster {} is not declared", name.get())))?;
    if pool.disabled() {
        return Err(Error::new(format!("cluster {} is disabled", name.get())));
    }
    let labels: Vec<Label> = pool
        .nodes()
        .iter()
        .map(|node| node.label().clone())
        .filter(|label| !manifest.drained(label))
        .collect();
    if labels.is_empty() {
        return Err(Error::new(format!(
            "cluster {} has no active nodes",
            name.get()
        )));
    }
    Ok(labels)
}

fn lookup<'a>(entries: &'a [Entry], label: &Label) -> Result<&'a Entry, Error> {
    entries
        .iter()
        .find(|entry| entry.field("name") == Some(label.get()))
        .ok_or_else(|| Error::new(format!("node {} is missing material", label.get())))
}

fn rotate(mut items: Vec<String>, slot: u64) -> Vec<String> {
    if items.len() > 1 {
        let offset = (slot as usize) % items.len();
        items.rotate_left(offset);
    }
    items
}
