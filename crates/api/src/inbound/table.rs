use dynet_core::{Cluster, Domain, Error, Name, Range, Rule, Subject, Table};
use std::path::Path;

const DIRECT: &str = "direct";

pub fn read(path: &Path, clusters: &[Cluster]) -> Result<Table, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    declare(&text, clusters)
}

pub fn declare(text: &str, clusters: &[Cluster]) -> Result<Table, Error> {
    let held: toml::Table = text
        .parse()
        .map_err(|error| Error::new(format!("the table does not parse: {error}")))?;
    let listed = held
        .get("rule")
        .and_then(toml::Value::as_array)
        .filter(|listed| !listed.is_empty())
        .ok_or_else(|| Error::new("a table must carry at least one rule"))?;
    let fallback = match held.get("fallback").and_then(toml::Value::as_str) {
        Some(named) => Name::new(named)?,
        None => Name::new(DIRECT)?,
    };
    settled(&fallback, clusters)?;
    let mut rules = Vec::new();
    for item in listed {
        rules.push(shaped(item, clusters)?);
    }
    Ok(Table::new(rules, fallback))
}

fn shaped(item: &toml::Value, clusters: &[Cluster]) -> Result<Rule, Error> {
    let named = item
        .get("cluster")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| Error::new("a rule carries no cluster"))?;
    let cluster = Name::new(named)?;
    if !clusters.iter().any(|held| held.name() == &cluster) {
        return Err(Error::new(format!(
            "a rule names cluster {named}, which is not declared"
        )));
    }
    Ok(Rule::new(subject(item)?, cluster))
}

fn subject(item: &toml::Value) -> Result<Subject, Error> {
    let mut held = Vec::new();
    if let Some(value) = worded(item, "exact")? {
        held.push(Subject::Exact(Domain::new(value)?));
    }
    if let Some(value) = worded(item, "suffix")? {
        held.push(Subject::Suffix(Domain::new(value)?));
    }
    if let Some(value) = worded(item, "holds")? {
        held.push(Subject::Holds(ranged(value)?));
    }
    match held.len() {
        1 => Ok(held.swap_remove(0)),
        0 => Err(Error::new(
            "a rule carries none of exact, suffix and holds; it must carry exactly one",
        )),
        _ => Err(Error::new(
            "a rule carries more than one of exact, suffix and holds",
        )),
    }
}

fn worded<'a>(item: &'a toml::Value, name: &str) -> Result<Option<&'a str>, Error> {
    match item.get(name) {
        None => Ok(None),
        Some(told) => told
            .as_str()
            .map(Some)
            .ok_or_else(|| Error::new(format!("a rule carries a {name} that is not text"))),
    }
}

fn ranged(claim: &str) -> Result<Range, Error> {
    let (base, prefix) = claim
        .split_once('/')
        .ok_or_else(|| Error::new(format!("{claim} carries no prefix length")))?;
    let base = base
        .parse()
        .map_err(|_| Error::new(format!("{base} is not an address")))?;
    let prefix = prefix
        .parse()
        .map_err(|_| Error::new(format!("{prefix} is not a prefix length")))?;
    Range::new(base, prefix)
}

fn settled(fallback: &Name, clusters: &[Cluster]) -> Result<(), Error> {
    if fallback.get() == DIRECT {
        return Ok(());
    }
    match clusters.iter().any(|held| held.name() == fallback) {
        true => Ok(()),
        false => Err(Error::new(format!(
            "the fallback names cluster {}, which is not declared",
            fallback.get()
        ))),
    }
}
