use dynet_api::inbound::table;
use dynet_api::{catalog, inbound, subscription};
use dynet_core::{Error, Instance, Router, Span};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    pub subscription: PathBuf,
    #[arg(long)]
    pub clusters: PathBuf,
    #[arg(long, default_value = "1.1.1.1")]
    pub upstream: String,
    #[arg(long, default_value = "0.0.0.0/0")]
    pub claim: String,
    #[arg(long)]
    pub bare: bool,
    #[arg(long)]
    pub unit: bool,
    #[arg(long)]
    pub table: PathBuf,
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
    let router = std::sync::Mutex::new(Router::new(table::read(&errand.table, held.clusters())?));
    let reload = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&reload))
        .map_err(|error| Error::new(format!("cannot listen for a reload: {error}")))?;
    let told = |line: &str| println!("  {line}");
    let warren = inbound::Warren {
        instance,
        entries: &entries,
        clusters: held.clusters(),
        router: &router,
        table: &errand.table,
        reload: &reload,
        port: stage.port,
        upstream: &errand.upstream,
        book: &dynet_api::outbound::Book::new(instance.mark()),
        store: &inbound::Store::new(),
        told: &told,
    };
    act(&warren)
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

fn told(name: &str, errand: &Errand) -> String {
    let mut parts = vec![
        format!("/usr/local/bin/dynet --instance {name} divert"),
        format!("--subscription {}", errand.subscription.display()),
        format!("--clusters {}", errand.clusters.display()),
        format!("--claim {}", errand.claim),
        format!("--upstream {}", errand.upstream),
    ];
    parts.push(format!("--table {}", errand.table.display()));
    parts.push("--seconds 0".to_string());
    parts.join(" ")
}

pub fn unit(name: &str, errand: &Errand) -> String {
    let bare = match errand.bare {
        true => " --bare",
        false => "",
    };
    let run = told(name, errand);
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
        "ExecReload=/bin/kill -HUP $MAINPID".to_string(),
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
