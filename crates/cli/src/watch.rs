use dynet_api::{catalog, inbound, subscription};
use dynet_core::{Domain, Error, Instance, Name, Range, Router, Rule, Span, Subject, Table};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    pub subscription: PathBuf,
    #[arg(long)]
    pub clusters: PathBuf,
    #[arg(long)]
    pub cluster: String,
    #[arg(long, default_value = "1.1.1.1")]
    pub upstream: String,
    #[arg(long, default_value = "0.0.0.0/0")]
    pub claim: String,
    #[arg(long)]
    pub bare: bool,
    #[arg(long)]
    pub unit: bool,
    #[arg(long, default_value = "")]
    pub holds: String,
    #[arg(long)]
    pub name: String,
    #[arg(long, default_value = "30")]
    pub seconds: u64,
}

pub struct Stage<'a> {
    pub errand: &'a Errand,
    pub port: u16,
    pub span: Span,
}

pub fn divert(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    if stage.errand.unit {
        print!("{}", unit(instance.get(), stage.errand));
        return Ok(ExitCode::SUCCESS);
    }
    let patience = std::time::Duration::from_secs(stage.errand.seconds);
    let ground = (stage.span, patience);
    let served = staged(instance, stage, |warren| inbound::divert(warren, ground))?;
    Ok(recount(&served))
}

fn staged<T>(
    instance: &Instance,
    stage: &Stage,
    act: impl FnOnce(&inbound::Warren) -> Result<T, Error>,
) -> Result<T, Error> {
    let errand = stage.errand;
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
    let router = std::sync::Mutex::new(Router::new(ruled(errand, wanted.name())?));
    let told = |line: &str| println!("  {line}");
    let warren = inbound::Warren {
        instance,
        entries: &entries,
        clusters: held.clusters(),
        router: &router,
        port: stage.port,
        upstream: &errand.upstream,
        book: &dynet_api::outbound::Book::new(instance.mark()),
        store: &inbound::Store::new(),
        told: &told,
    };
    act(&warren)
}

fn ruled(errand: &Errand, wanted: &Name) -> Result<Table, Error> {
    let mut rules = Vec::new();
    for name in split(&errand.name) {
        rules.push(Rule::new(
            Subject::Suffix(Domain::new(name)?),
            wanted.clone(),
        ));
    }
    for held in split(&errand.holds) {
        rules.push(Rule::new(Subject::Holds(span(held)?), wanted.clone()));
    }
    Ok(Table::new(rules, Name::new("direct")?))
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
        "accepted {}, answered {}, faulted {}, named {}, refused {}, dropped {}",
        served.accepted,
        served.answered,
        served.faulted,
        served.named,
        served.refused,
        served.dropped
    );
    match served.accepted > 0 && served.faulted == 0 && served.named > 0 {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    }
}

pub fn unit(name: &str, errand: &Errand) -> String {
    let bare = match errand.bare {
        true => " --bare",
        false => "",
    };
    let run = format!(
        "/usr/local/bin/dynet --instance {name} divert --subscription {} --clusters {} --cluster {} --claim {} --upstream {} --name {} --seconds 0",
        errand.subscription.display(),
        errand.clusters.display(),
        errand.cluster,
        errand.claim,
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
            "ExecStartPre=/usr/local/bin/dynet --instance {name} up --claim {}{bare}",
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
