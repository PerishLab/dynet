use dynet_api::{inbound, manifest};
use dynet_core::{Error, Instance, Ledger, Span};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    pub manifest: PathBuf,
    #[arg(long, default_value = "1.1.1.1")]
    pub upstream: String,
    #[arg(long, default_value = "0.0.0.0/0")]
    pub claim: String,
    #[arg(long)]
    pub bare: bool,
    #[arg(long)]
    pub unit: bool,
    #[arg(long, default_value = "30")]
    pub seconds: u64,
}

pub struct Stage<'a> {
    pub errand: &'a Errand,
    pub port: u16,
    pub span: Span,
    pub transit: u16,
}

pub fn divert(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    if stage.errand.unit {
        print!("{}", unit(instance.get(), stage.errand));
        return Ok(ExitCode::SUCCESS);
    }
    let patience = std::time::Duration::from_secs(stage.errand.seconds);
    let ground = (stage.span, patience, stage.transit);
    let served = staged(instance, stage, |warren| inbound::divert(warren, ground))?;
    Ok(recount(&served))
}

fn staged<T>(
    instance: &Instance,
    stage: &Stage,
    act: impl FnOnce(&inbound::Warren) -> Result<T, Error>,
) -> Result<T, Error> {
    let errand = stage.errand;
    let loaded = manifest::read(&errand.manifest)?;
    let held = std::sync::RwLock::new(Arc::new(inbound::Held::new(loaded, None)?));
    let ledger = std::sync::Mutex::new(Ledger::new());
    let reload = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&reload))
        .map_err(|error| Error::new(format!("cannot listen for a reload: {error}")))?;
    let told = |line: &str| println!("  {line}");
    let warren = inbound::Warren {
        instance,
        held: &held,
        ledger: &ledger,
        source: &errand.manifest,
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
        format!("--manifest {}", errand.manifest.display()),
        format!("--claim {}", errand.claim),
        format!("--upstream {}", errand.upstream),
    ];
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
