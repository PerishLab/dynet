use crate::subscription::Entry;
use dynet_core::{Capability, Carriage, Cluster, Error, Label, Name, Node};
use std::collections::BTreeMap;

const CARRIED: &str = "vmess";
const FIRST: u32 = 0x1F1E6;
const LAST: u32 = 0x1F1FF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Grouped,
    Protocol,
    Placeless,
    Repeated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Declined {
    label: Label,
    reason: Reason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Catalog {
    clusters: Vec<Cluster>,
    declined: Vec<Declined>,
}

impl Declined {
    pub fn label(&self) -> &Label {
        &self.label
    }

    pub fn reason(&self) -> Reason {
        self.reason
    }
}

impl Catalog {
    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    pub fn declined(&self) -> &[Declined] {
        &self.declined
    }

    pub fn nodes(&self) -> usize {
        self.clusters.iter().map(|item| item.nodes().len()).sum()
    }
}

pub fn build(entries: &[Entry]) -> Result<Catalog, Error> {
    let mut grouped: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    let mut declined = Vec::new();
    let mut seen = Vec::new();
    for entry in entries {
        let label = Label::new(entry.field("name").unwrap_or_default())?;
        match sort(entry, &label, &seen) {
            Err(reason) => declined.push(Declined { label, reason }),
            Ok(place) => {
                seen.push(label.clone());
                grouped.entry(place).or_default().push(node(entry, label));
            }
        }
    }
    let mut clusters = Vec::new();
    for (place, nodes) in grouped {
        clusters.push(Cluster::new(Name::new(place)?, nodes)?);
    }
    Ok(Catalog { clusters, declined })
}

fn node(entry: &Entry, label: Label) -> Node {
    let carriage = match entry.flag("udp") {
        true => Carriage::Native,
        false => Carriage::Absent,
    };
    Node::new(label, Capability::new(carriage))
}

fn sort(entry: &Entry, label: &Label, seen: &[Label]) -> Result<String, Reason> {
    if entry.field("server").is_none() || entry.field("port").is_none() {
        return Err(Reason::Grouped);
    }
    if entry.field("type") != Some(CARRIED) {
        return Err(Reason::Protocol);
    }
    let Some(place) = place(label.get()) else {
        return Err(Reason::Placeless);
    };
    if seen.contains(label) {
        return Err(Reason::Repeated);
    }
    Ok(place)
}

fn place(label: &str) -> Option<String> {
    let mut characters = label.chars();
    let leading = indicator(characters.next()?)?;
    let trailing = indicator(characters.next()?)?;
    Some(format!("{leading}{trailing}"))
}

fn indicator(value: char) -> Option<char> {
    let point = u32::from(value);
    if !(FIRST..=LAST).contains(&point) {
        return None;
    }
    char::from_u32(point - FIRST + u32::from(b'a'))
}
