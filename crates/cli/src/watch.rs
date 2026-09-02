use dynet_api::{catalog, inbound, subscription};
use dynet_core::{Domain, Error, Instance, Name, Range, Router, Rule, Subject, Table};
use std::path::PathBuf;
use std::process::ExitCode;

pub struct Errand {
    pub subscription: PathBuf,
    pub clusters: PathBuf,
    pub cluster: String,
    pub ports: String,
    pub upstream: String,
    pub claim: String,
    pub under: Option<String>,
    pub unit: bool,
    pub holds: String,
    pub port: u16,
    pub seconds: u64,
    pub name: String,
}

pub fn forward(instance: &Instance, errand: &Errand) -> Result<ExitCode, Error> {
    if errand.unit {
        print!("{}", unit(instance, errand));
        return Ok(ExitCode::SUCCESS);
    }
    let text = std::fs::read_to_string(&errand.subscription).map_err(|error| {
        Error::new(format!(
            "cannot read {}: {error}",
            errand.subscription.display()
        ))
    })?;
    let mut entries = subscription::read(&text)?;
    let spoken = std::fs::read_to_string(&errand.clusters).map_err(|error| {
        Error::new(format!(
            "cannot read {}: {error}",
            errand.clusters.display()
        ))
    })?;
    let held = catalog::declare(&spoken, &entries)?;
    entries.extend(held.spoken().iter().cloned());
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
        store: &inbound::Store::new(),
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

pub fn unit(instance: &Instance, errand: &Errand) -> String {
    let name = instance.get();
    let under = match &errand.under {
        Some(owner) => format!(" --under {owner}"),
        None => String::new(),
    };
    let run = format!(
        "/usr/local/bin/dynet --instance {name} forward --subscription {} --clusters {} --cluster {} --claim {} --ports {} --upstream {} --name {} --seconds 0",
        errand.subscription.display(),
        errand.clusters.display(),
        errand.cluster,
        errand.claim,
        errand.ports,
        errand.upstream,
        errand.name,
    );
    [
        "[Unit]".to_string(),
        format!("Description=Dynet on {name}"),
        "Wants=network-online.target".to_string(),
        "After=network-online.target systemd-resolved.service".to_string(),
        "StartLimitIntervalSec=0".to_string(),
        String::new(),
        "[Service]".to_string(),
        "Type=simple".to_string(),
        format!("ExecStartPre=-/usr/local/bin/dynet --instance {name} down"),
        format!(
            "ExecStartPre=/usr/local/bin/dynet --instance {name} up --claim {}{under}",
            errand.claim
        ),
        format!("ExecStart={run}"),
        format!("ExecStopPost=-/usr/local/bin/dynet --instance {name} down"),
        "Restart=always".to_string(),
        "RestartSec=5".to_string(),
        String::new(),
        "[Install]".to_string(),
        "WantedBy=multi-user.target".to_string(),
        String::new(),
    ]
    .join("\n")
}
