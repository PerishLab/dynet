use dynet_api::host::Journal;
use dynet_api::{control, inbound};
use dynet_core::{Error, Instance, Span};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Clone, clap::Args)]
pub struct Errand {
    #[arg(long)]
    pub bootstrap: PathBuf,
    #[arg(long, default_value = "0.0.0.0/0")]
    pub claim: String,
    #[arg(long)]
    pub bare: bool,
    #[arg(long)]
    pub unit: bool,
    #[arg(long, default_value = "30")]
    pub seconds: u64,
    #[arg(long)]
    pub service: bool,
}

pub struct Stage<'a> {
    pub errand: &'a Errand,
    pub state: &'a dynet_api::host::State,
    pub port: u16,
    pub span: Span,
    pub transit: u16,
}

pub fn divert(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    if stage.errand.unit {
        #[cfg(not(target_os = "macos"))]
        print!("{}", unit(instance.get(), stage.errand, stage.state));
        #[cfg(target_os = "macos")]
        print!("{}", crate::platform::unit(instance, stage)?);
        return Ok(ExitCode::SUCCESS);
    }
    if stage.errand.service {
        return served(instance, stage);
    }
    let halt = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&halt))
            .map_err(|error| Error::new(format!("cannot listen for a halt: {error}")))?;
    }
    let told = |line: &str| println!("  {line}");
    let served = carried(instance, stage, (&halt, &told))?;
    Ok(recount(&served))
}

fn served(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    #[cfg(any(windows, target_os = "macos"))]
    return crate::platform::serve(instance, stage);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (instance, stage);
        Err(Error::new(
            "--service runs under the native service manager; on Linux install --unit",
        ))
    }
}

pub fn carried(
    instance: &Instance,
    stage: &Stage,
    told: (&AtomicBool, &(dyn Fn(&str) + Sync)),
) -> Result<inbound::Served, Error> {
    let errand = stage.errand;
    let (halt, told) = told;
    let patience = std::time::Duration::from_secs(errand.seconds);
    let ground = (stage.span, patience, stage.transit);
    let session = control::Session::open(
        &errand.bootstrap,
        instance,
        stage.state,
        control::Private::machine(),
    )?;
    let payload = session
        .initial
        .payload
        .clone()
        .or_else(|| session.protection.clone())
        .unwrap_or(control::Payload {
            loaded: dynet_api::manifest::load(
                r#"{"version":1,"nodes":[],"clusters":[],"rules":[],"fallback":"direct"}"#,
            )?,
            upstream: std::net::Ipv4Addr::new(1, 1, 1, 1),
            retain: 86400,
        });
    let retain = payload.retain;
    let mut active = inbound::Held::new(payload, None)?;
    active.stamp = session.initial.assignment.receipt.digest.clone();
    if session.initial.payload.is_none() {
        active.fence.close()?;
        active.paused = true;
    }
    let held = std::sync::RwLock::new(Arc::new(active));
    let journal = Journal::controlled(instance, stage.state, Arc::clone(&session.control.cache));
    let mut issued = journal.load(retain)?;
    if session.initial.payload.is_some() {
        issued.rejudge(
            held.read()
                .map_err(|_| Error::new("the policy is poisoned"))?
                .loaded
                .manifest(),
        );
    }
    let ledger = std::sync::Mutex::new(issued);
    let reload = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&reload))
        .map_err(|error| Error::new(format!("cannot listen for a reload: {error}")))?;
    let warren = inbound::Warren {
        instance,
        initial: &session.initial.assignment,
        held: &held,
        ledger: &ledger,
        journal: &journal,
        control: &session.control,
        projection: instance,
        reload: &reload,
        halt,
        port: stage.port,
        book: &dynet_api::outbound::Book::new(instance.mark()),
        store: &inbound::Store::new(),
        told,
    };
    inbound::divert(&warren, ground)
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

#[cfg(not(target_os = "macos"))]
fn told(name: &str, errand: &Errand, state: &dynet_api::host::State) -> String {
    let mut parts = vec![
        format!(
            "/usr/local/bin/dynet --instance {name}{} divert",
            located(state)
        ),
        format!("--bootstrap {}", errand.bootstrap.display()),
        format!("--claim {}", errand.claim),
    ];
    parts.push("--seconds 0".to_string());
    parts.join(" ")
}

#[cfg(not(target_os = "macos"))]
pub fn unit(name: &str, errand: &Errand, state: &dynet_api::host::State) -> String {
    let bare = match errand.bare {
        true => " --bare",
        false => "",
    };
    let run = told(name, errand, state);
    [
        "[Unit]".to_string(),
        format!("Description=Dynet on {name}"),
        "Wants=network-online.target".to_string(),
        "After=network-online.target systemd-resolved.service".to_string(),
        "StartLimitIntervalSec=0".to_string(),
        String::new(),
        "[Service]".to_string(),
        "Type=simple".to_string(),
        "StateDirectory=dynet".to_string(),
        "RuntimeDirectory=dynet".to_string(),
        "RuntimeDirectoryPreserve=yes".to_string(),
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

#[cfg(not(target_os = "macos"))]
fn located(state: &dynet_api::host::State) -> String {
    let Some(path) = state.configured() else {
        return String::new();
    };
    let path = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!(" --state \"{path}\"")
}
