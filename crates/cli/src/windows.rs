use crate::Config;
use crate::watch::{self, Errand, Stage};
use dynet_api::host::{self, Journal, clock};
use dynet_core::{Error, Instance, Span};
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Mutex;

#[derive(clap::Subcommand)]
pub(crate) enum Service {
    Install(Errand),
    Remove,
}

pub(crate) struct Boundary(pub(crate) Instance);

impl Boundary {
    pub(crate) fn doctor(&self, config: &Config) -> Result<ExitCode, Error> {
        let seat = Span::new(&config.span)?.seat();
        let ruled = host::Veil::ruled(&self.0);
        let walled = host::Veil::walled(&self.0);
        let held = host::Veil::attended(seat, config.port);
        println!("instance {}", self.0.get());
        println!("  device: {}", present(host::Link::present(&self.0)));
        println!("  names: {}", present(ruled));
        println!("  wall: {}", present(walled));
        println!("  resolver: {}", attending(held));
        if (ruled || walled) && !held {
            println!(
                "stranded: names and claimed destinations fail closed and nothing answers behind them"
            );
            return Ok(ExitCode::from(1));
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn raise(
        &self,
        config: &Config,
        claim: &str,
        bare: bool,
    ) -> Result<ExitCode, Error> {
        let ground = host::Ground {
            port: config.port,
            claim,
            span: Span::new(&config.span)?,
            bare,
            sources: &[],
            transit: config.transit,
        };
        let standing = host::establish(&self.0, &ground)?;
        println!(
            "up: reclaimed={} veiled={}",
            standing.cleared.any(),
            standing.veiled
        );
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn lower(&self) -> Result<ExitCode, Error> {
        self.stop()?;
        let cleared = host::reclaim(&self.0)?;
        println!("down: veil={}", cleared.veil);
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn reading(&self) -> Result<ExitCode, Error> {
        let path = dynet_api::inbound::standing::path(&self.0);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
        for line in text.lines().filter(|line| *line != host::MARKER) {
            println!("{line}");
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn service(&self, config: &Config, service: &Service) -> Result<ExitCode, Error> {
        match service {
            Service::Install(errand) => self.install(config, errand),
            Service::Remove => self.remove(),
        }
    }

    fn install(&self, config: &Config, errand: &Errand) -> Result<ExitCode, Error> {
        if !config.sources.is_empty() || config.transit != 0 {
            return Err(Error::new(
                "capturing forwarded traffic is not offered on Windows",
            ));
        }
        if errand.bare || errand.unit {
            return Err(Error::new(
                "a Windows service must establish its own protected host",
            ));
        }
        Span::new(&config.span)?;
        let program = host::program()?;
        let manifest = std::fs::canonicalize(&errand.manifest).map_err(|error| {
            Error::new(format!(
                "cannot place {}: {error}",
                errand.manifest.display()
            ))
        })?;
        let name = self.0.get();
        let mut line = format!(
            "\"{}\" --instance {name} --port {} --span {} divert --service --manifest \"{}\" --upstream {} --claim {} --retain {} --seconds 0",
            program.display(),
            config.port,
            config.span,
            plain(&manifest),
            quoted(&errand.upstream),
            quoted(&errand.claim),
            errand.retain
        );
        if let Some(ledger) = &errand.ledger {
            let ledger = std::path::absolute(ledger)
                .map_err(|error| Error::new(format!("cannot place the ledger: {error}")))?;
            line.push_str(&format!(" --ledger \"{}\"", ledger.display()));
        }
        let display = format!("Dynet {name}");
        host::run(
            "sc.exe",
            &[
                "create",
                name,
                "binPath=",
                &line,
                "start=",
                "auto",
                "DisplayName=",
                &display,
            ],
        )?;
        let actions = "restart/5000/restart/5000/restart/5000";
        host::run(
            "sc.exe",
            &["failure", name, "reset=", "86400", "actions=", actions],
        )?;
        host::run("sc.exe", &["failureflag", name, "1"])?;
        host::run(
            "sc.exe",
            &["description", name, "Dynet captures claimed traffic"],
        )?;
        println!("installed {name}: {line}");
        Ok(ExitCode::SUCCESS)
    }

    fn remove(&self) -> Result<ExitCode, Error> {
        let name = self.0.get();
        self.stop()?;
        host::reclaim(&self.0)?;
        host::run("sc.exe", &["delete", name])?;
        println!("removed {name}");
        Ok(ExitCode::SUCCESS)
    }

    fn stop(&self) -> Result<(), Error> {
        let name = self.0.get();
        let state = match host::run("sc.exe", &["query", name]) {
            Ok(state) => state,
            Err(error) if error.to_string().contains("1060") => return Ok(()),
            Err(error) => return Err(error),
        };
        if state.contains("STOPPED") {
            return Ok(());
        }
        host::run("sc.exe", &["stop", name])?;
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(30) {
            let state = host::run("sc.exe", &["query", name])?;
            if state.contains("STOPPED") {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Err(Error::new(format!(
            "service {name} did not stop within 30 seconds"
        )))
    }
}

fn quoted(value: &str) -> String {
    let mut quoted = String::from("\"");
    let mut slashes = 0;
    for character in value.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        let count = if character == '"' {
            slashes * 2 + 1
        } else {
            slashes
        };
        quoted.extend(std::iter::repeat_n('\\', count));
        quoted.push(character);
        slashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

pub(crate) fn serve(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    let instance = instance.clone();
    let errand = stage.errand.clone();
    let (port, span) = (stage.port, stage.span);
    let name = instance.get().to_string();
    dynet_windows::serve(&name, move || {
        let stage = Stage {
            errand: &errand,
            port,
            span,
            transit: 0,
        };
        work(&instance, &stage)
    })
    .map_err(|error| Error::new(format!("the service was not served: {error}")))?;
    Ok(ExitCode::SUCCESS)
}

fn work(instance: &Instance, stage: &Stage) -> u32 {
    let log = Journal::new(instance, None).path().with_extension("log");
    let Some(parent) = log.parent() else { return 1 };
    if std::fs::create_dir_all(parent).is_err() {
        return 1;
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        Ok(file) => file,
        Err(_) => return 1,
    };
    let file = Mutex::new(file);
    let told = |line: &str| {
        if let Ok(mut held) = file.lock() {
            let _ = writeln!(held, "{} {line}", clock());
        }
    };
    let ground = host::Ground {
        port: stage.port,
        claim: &stage.errand.claim,
        span: stage.span,
        bare: stage.errand.bare,
        sources: &[],
        transit: 0,
    };
    told(&format!(
        "starting {} {}",
        instance.get(),
        dynet_api::VERSION
    ));
    if let Err(error) = host::establish(instance, &ground) {
        told(&format!("refused to raise: {error}"));
        return 1;
    }
    let carried = watch::carried(instance, stage, (dynet_windows::halted(), &told));
    let code = match &carried {
        Ok(_) => 0,
        Err(error) => {
            told(&format!(
                "diverting failed, claimed destinations stay closed: {error}"
            ));
            1
        }
    };
    if carried.is_ok() && !dynet_windows::closing() {
        match host::reclaim(instance) {
            Ok(cleared) => told(&format!("stopped and lowered: veil={}", cleared.veil)),
            Err(error) => told(&format!("stopped but not lowered: {error}")),
        }
    }
    code
}

fn plain(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()
}

fn present(held: bool) -> &'static str {
    match held {
        true => "present",
        false => "absent",
    }
}

fn attending(held: bool) -> &'static str {
    match held {
        true => "answering",
        false => "silent",
    }
}
