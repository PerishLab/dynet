mod worker;

pub(crate) use worker::serve;

use crate::Config;
use crate::watch::Errand;
use dynet_api::host;
use dynet_core::{Error, Instance, Span};
use std::path::Path;
use std::process::ExitCode;

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
            local: config.local,
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

    pub(crate) fn reading(&self, state: &host::State) -> Result<ExitCode, Error> {
        let path = dynet_api::inbound::standing::path(&self.0, state);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
        for line in text.lines().filter(|line| *line != host::MARKER) {
            println!("{line}");
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn service(
        &self,
        config: &Config,
        service: &Service,
        state: &host::State,
    ) -> Result<ExitCode, Error> {
        match service {
            Service::Install(errand) => self.install(config, errand, state),
            Service::Remove => self.remove(),
        }
    }

    fn install(
        &self,
        config: &Config,
        errand: &Errand,
        state: &host::State,
    ) -> Result<ExitCode, Error> {
        config.validate()?;
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
        let bootstrap = std::path::absolute(&errand.bootstrap)
            .map_err(|_| Error::new("cannot place the local bootstrap"))?;
        let private = dynet_api::control::Private::machine();
        private.folder(state.root()).map_err(|_| {
            Error::new(
                "the service state directory must be private to administrators and LocalSystem",
            )
        })?;
        dynet_api::control::Bootstrap::read(&bootstrap, private)?
            .inspect(private, self.0.mark())?;
        let name = self.0.get();
        let line = format!(
            "\"{}\" --instance {name} --port {} --span {} --state {} divert --service --bootstrap {} --claim {} --seconds 0",
            program.display(),
            config.port,
            config.span,
            quoted(&plain(state.root())),
            quoted(&plain(&bootstrap)),
            quoted(&errand.claim),
        );
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
