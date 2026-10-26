use super::wire::Pool;
use super::{Cluster, Declared, Form, Label, Name, Node};
use crate::Affinity;
use crate::error::Error;
use std::collections::BTreeSet;

pub(super) fn clusters(
    listed: &[Pool],
    nodes: &[Declared],
    drained: &BTreeSet<Label>,
) -> Result<Vec<Cluster>, Error> {
    let mut clusters: Vec<Cluster> = Vec::new();
    for pool in listed {
        let cluster = shaped(pool, nodes, drained)?;
        if clusters.iter().any(|held| held.name() == cluster.name()) {
            return Err(Error::new(format!(
                "cluster {} is declared twice",
                pool.name
            )));
        }
        clusters.push(cluster);
    }
    for cluster in &clusters {
        settled(cluster, &clusters)?;
    }
    Ok(clusters)
}

fn shaped(pool: &Pool, nodes: &[Declared], drained: &BTreeSet<Label>) -> Result<Cluster, Error> {
    let name = Name::new(&pool.name)?;
    let form = form(pool, nodes, drained)?;
    let alias = matches!(form, Form::Alias(_));
    let pooled = matches!(form, Form::Pool(_));
    let decorated = pool.disabled || pool.bound.is_some() || pool.probe.is_some();
    if alias && decorated {
        return Err(Error::new(format!(
            "alias {} carries something besides its pool",
            pool.name
        )));
    }
    if !pooled && (pool.bound.is_some() || pool.probe.is_some()) {
        return Err(Error::new(format!(
            "cluster {} is not a pool and cannot carry bound or probe_exit",
            pool.name
        )));
    }
    let mut cluster = Cluster::new(name, form)?;
    if let Some(told) = &pool.bound {
        cluster = cluster.bounded(Affinity::read(told)?);
    }
    if let Some(told) = &pool.probe {
        cluster = cluster.probing(known(told, nodes)?);
    }
    if pool.disabled {
        cluster = cluster.disable();
    }
    Ok(cluster)
}

fn form(pool: &Pool, nodes: &[Declared], drained: &BTreeSet<Label>) -> Result<Form, Error> {
    let listed = pool.nodes.is_some() || pool.members.is_some();
    let fronted = pool.via.is_some() || pool.exit.is_some();
    let spoken = [listed, pool.pool.is_some(), fronted];
    if spoken.iter().filter(|held| **held).count() != 1 {
        return Err(Error::new(format!(
            "cluster {} must list nodes or members, or be a pool alias, or carry via with exit; exactly one",
            pool.name
        )));
    }
    if let Some(target) = &pool.pool {
        return Ok(Form::Alias(Name::new(target)?));
    }
    match (&pool.via, &pool.exit) {
        (Some(front), Some(exit)) => Ok(Form::Via(Name::new(front)?, known(exit, nodes)?)),
        (None, None) => Ok(Form::Pool(gathered(pool, nodes, drained)?)),
        _ => Err(Error::new(format!(
            "cluster {} carries via and exit only together",
            pool.name
        ))),
    }
}

fn gathered(
    pool: &Pool,
    nodes: &[Declared],
    drained: &BTreeSet<Label>,
) -> Result<Vec<Node>, Error> {
    let mut wanted: Vec<Label> = Vec::new();
    for told in pool.nodes.iter().flatten() {
        wanted.push(known(told, nodes)?);
    }
    for member in pool.members.iter().flatten() {
        let named: Vec<&Declared> = nodes
            .iter()
            .filter(|node| node.provider() == member.provider)
            .collect();
        if named.is_empty() {
            return Err(Error::new(format!(
                "cluster {} takes provider {}, which declares no node",
                pool.name, member.provider
            )));
        }
        wanted.extend(named.into_iter().map(|node| node.label().clone()));
    }
    if wanted.is_empty() {
        return Err(Error::new(format!("cluster {} lists no node", pool.name)));
    }
    let mut seen = BTreeSet::new();
    let mut gathered = Vec::new();
    for label in wanted {
        if drained.contains(&label) || !seen.insert(label.clone()) {
            continue;
        }
        let capability = nodes
            .iter()
            .find(|node| node.label() == &label)
            .map(Declared::capability)
            .ok_or_else(|| Error::new(format!("node {} is not declared", label.get())))?;
        gathered.push(Node::new(label, capability));
    }
    Ok(gathered)
}

fn known(told: &str, nodes: &[Declared]) -> Result<Label, Error> {
    let label = Label::new(told)?;
    match nodes.iter().any(|node| node.label() == &label) {
        true => Ok(label),
        false => Err(Error::new(format!("node {told} is named but not declared"))),
    }
}

fn settled(cluster: &Cluster, clusters: &[Cluster]) -> Result<(), Error> {
    let target = match cluster.form() {
        Form::Alias(target) | Form::Via(target, _) => target,
        Form::Pool(_) => return probed(cluster, clusters),
    };
    let found = clusters.iter().find(|held| held.name() == target);
    match found.map(Cluster::form) {
        Some(Form::Pool(_)) => Ok(()),
        Some(_) => Err(Error::new(format!(
            "cluster {} leads to {}, which is not a pool; aliases and fronts are one level",
            cluster.name().get(),
            target.get()
        ))),
        None => Err(Error::new(format!(
            "cluster {} leads to {}, which is not declared",
            cluster.name().get(),
            target.get()
        ))),
    }
}

fn probed(cluster: &Cluster, clusters: &[Cluster]) -> Result<(), Error> {
    let Some(exit) = cluster.probe() else {
        return Ok(());
    };
    let fronted = clusters.iter().any(|held| {
        matches!(held.form(), Form::Via(front, out) if front == cluster.name() && out == exit)
    });
    match fronted {
        true => Ok(()),
        false => Err(Error::new(format!(
            "pool {} probes through {}, which is not the exit of any cluster fronted by it",
            cluster.name().get(),
            exit.get()
        ))),
    }
}
