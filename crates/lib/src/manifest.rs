mod capability;
mod cluster;
mod label;
mod shape;
mod wire;

pub use capability::{Bearing, Capability, Carriage};
pub use cluster::{Cluster, DIRECT, Form, Name, Node};
pub use label::Label;

use crate::error::Error;
use crate::routing::{Domain, Range, Rule, Subject, Table};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 1;
const SPOKEN: [&str; 5] = ["label", "provider", "region", "alias", "protocol"];

#[derive(Clone, Eq, PartialEq)]
pub struct Declared {
    label: Label,
    provider: String,
    protocol: String,
    material: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    generation: Option<String>,
    nodes: Vec<Declared>,
    clusters: Vec<Cluster>,
    table: Table,
    drained: BTreeSet<Label>,
}

impl Declared {
    pub fn label(&self) -> &Label {
        &self.label
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.material.get(name).map(String::as_str)
    }

    pub fn material(&self) -> &BTreeMap<String, String> {
        &self.material
    }

    pub fn capability(&self) -> Capability {
        let relayed = self.protocol == "vmess" && self.field("udp") == Some("true");
        Capability::new(match relayed {
            true => Carriage::Relay,
            false => Carriage::Absent,
        })
    }
}

impl Manifest {
    pub fn read(text: &str) -> Result<Self, Error> {
        let wire: wire::Wire = serde_json::from_str(text)
            .map_err(|error| Error::new(format!("the manifest does not parse: {error}")))?;
        if wire.version != VERSION {
            return Err(Error::new(format!(
                "manifest version {} is not the version {VERSION} this reads",
                wire.version
            )));
        }
        let nodes = declare(&wire.nodes)?;
        let drained = drained(&wire.drain, &nodes)?;
        let clusters = shape::clusters(&wire.clusters, &nodes, &drained)?;
        let table = table(&wire.rules, &wire.fallback, &clusters)?;
        Ok(Self {
            generation: wire.generation,
            nodes,
            clusters,
            table,
            drained,
        })
    }

    pub fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }

    pub fn nodes(&self) -> &[Declared] {
        &self.nodes
    }

    pub fn node(&self, label: &Label) -> Option<&Declared> {
        self.nodes.iter().find(|node| node.label() == label)
    }

    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    pub fn cluster(&self, name: &Name) -> Option<&Cluster> {
        self.clusters.iter().find(|cluster| cluster.name() == name)
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    pub fn drained(&self, label: &Label) -> bool {
        self.drained.contains(label)
    }

    pub fn pool(&self, name: &Name) -> Option<&Cluster> {
        let cluster = self.cluster(name)?;
        match cluster.form() {
            Form::Alias(target) => self.cluster(target),
            Form::Pool(_) | Form::Via(..) => Some(cluster),
        }
    }
}

fn declare(listed: &[Map<String, Value>]) -> Result<Vec<Declared>, Error> {
    let mut nodes: Vec<Declared> = Vec::new();
    for fields in listed {
        let node = declared(fields)?;
        if nodes.iter().any(|held| held.label == node.label) {
            return Err(Error::new(format!(
                "node {} is declared twice",
                node.label.get()
            )));
        }
        nodes.push(node);
    }
    Ok(nodes)
}

fn declared(fields: &Map<String, Value>) -> Result<Declared, Error> {
    let text = |name: &str| fields.get(name).and_then(Value::as_str);
    let label = Label::new(text("label").ok_or_else(|| Error::new("a node carries no label"))?)?;
    let protocol = text("protocol")
        .ok_or_else(|| Error::new(format!("node {} carries no protocol", label.get())))?
        .to_string();
    let provider = text("provider")
        .map(str::to_string)
        .unwrap_or_else(|| host(label.get()));
    let mut flat = Vec::new();
    for (name, value) in fields {
        if !SPOKEN.contains(&name.as_str()) {
            wire::flatten(name, value, &mut flat);
        }
    }
    Ok(Declared {
        label,
        provider,
        protocol,
        material: flat.into_iter().collect(),
    })
}

fn host(label: &str) -> String {
    label.split('/').next().unwrap_or(label).to_string()
}

fn drained(drain: &wire::Drain, nodes: &[Declared]) -> Result<BTreeSet<Label>, Error> {
    let mut held = BTreeSet::new();
    for told in &drain.nodes {
        let label = Label::new(told)?;
        if !nodes.iter().any(|node| node.label == label) {
            return Err(Error::new(format!(
                "drain names node {told}, which is not declared"
            )));
        }
        held.insert(label);
    }
    for provider in &drain.providers {
        let named: Vec<&Declared> = nodes
            .iter()
            .filter(|node| &node.provider == provider)
            .collect();
        if named.is_empty() {
            return Err(Error::new(format!(
                "drain names provider {provider}, which declares no node"
            )));
        }
        held.extend(named.into_iter().map(|node| node.label.clone()));
    }
    Ok(held)
}

fn table(lines: &[wire::Line], fallback: &str, clusters: &[Cluster]) -> Result<Table, Error> {
    let known = |told: &str| -> Result<Name, Error> {
        let name = Name::new(told)?;
        match clusters.iter().any(|cluster| cluster.name() == &name) {
            true => Ok(name),
            false => Err(Error::new(format!(
                "cluster {told} is named but not declared"
            ))),
        }
    };
    let mut rules = Vec::new();
    for line in lines {
        rules.push(Rule::new(subject(line)?, known(&line.cluster)?));
    }
    let fallback = match fallback {
        DIRECT => None,
        told => Some(known(told)?),
    };
    Table::new(rules, fallback)
}

fn subject(line: &wire::Line) -> Result<Subject, Error> {
    match (&line.exact, &line.suffix, &line.holds) {
        (Some(name), None, None) => Ok(Subject::Exact(Domain::new(name)?)),
        (None, Some(name), None) => Ok(Subject::Suffix(Domain::new(name)?)),
        (None, None, Some(range)) => Ok(Subject::Holds(Range::read(range)?)),
        _ => Err(Error::new(format!(
            "a rule for cluster {} must carry exactly one of exact, suffix and holds",
            line.cluster
        ))),
    }
}

impl std::fmt::Debug for Declared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Declared([redacted])")
    }
}
