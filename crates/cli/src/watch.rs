use dynet_api::{catalog, inbound, subscription};
use dynet_core::{Domain, Error, Instance, Name, Range, Router, Rule, Subject, Table};
use std::path::PathBuf;
use std::process::ExitCode;

pub struct Errand {
    pub subscription: PathBuf,
    pub cluster: String,
    pub ports: String,
    pub upstream: String,
    pub holds: String,
    pub port: u16,
    pub seconds: u64,
    pub name: String,
}

pub fn forward(instance: &Instance, errand: &Errand) -> Result<ExitCode, Error> {
    let text = std::fs::read_to_string(&errand.subscription).map_err(|error| {
        Error::new(format!(
            "cannot read {}: {error}",
            errand.subscription.display()
        ))
    })?;
    let entries = subscription::read(&text)?;
    let held = catalog::build(&entries)?;
    let wanted = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == errand.cluster)
        .ok_or_else(|| Error::new(format!("no cluster named {}", errand.cluster)))?;
    let mut rules = Vec::new();
    for name in split(&errand.name) {
        rules.push(Rule::new(
            Subject::Suffix(Domain::new(name)?),
            wanted.name().clone(),
        ));
    }
    for held in split(&errand.holds) {
        rules.push(Rule::new(
            Subject::Holds(span(held)?),
            wanted.name().clone(),
        ));
    }
    let table = Table::new(rules, Name::new("direct")?);
    let router = std::sync::Mutex::new(Router::new(table));
    let told = |line: &str| println!("  {line}");
    let warren = inbound::Warren {
        instance,
        entries: &entries,
        clusters: held.clusters(),
        router: &router,
        ports: &listed(&errand.ports)?,
        port: errand.port,
        upstream: &errand.upstream,
        book: &dynet_api::outbound::Book::new(),
        told: &told,
    };
    let served = inbound::serve(&warren, std::time::Duration::from_secs(errand.seconds))?;
    Ok(recount(&served))
}

pub fn listed(ports: &str) -> Result<Vec<u16>, Error> {
    ports
        .split(',')
        .map(|item| {
            item.trim()
                .parse()
                .map_err(|_| Error::new(format!("{item} is not a port")))
        })
        .collect()
}

pub fn split(listed: &str) -> Vec<&str> {
    listed
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn span(claim: &str) -> Result<Range, Error> {
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

pub fn recount(served: &inbound::Served) -> ExitCode {
    println!(
        "accepted {}, answered {}, faulted {}, named {}, refused {}",
        served.accepted, served.answered, served.faulted, served.named, served.refused
    );
    match served.accepted > 0 && served.faulted == 0 && served.named > 0 {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    }
}
