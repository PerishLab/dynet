mod service;
mod worker;

pub(crate) use service::{Service, unit};
pub(crate) use worker::serve;

use crate::Config;
use dynet_api::host;
use dynet_core::{Error, Instance, Span};
use std::process::ExitCode;

pub(crate) struct Boundary(pub(crate) Instance);

impl Boundary {
    pub(crate) fn doctor(&self, config: &Config) -> Result<ExitCode, Error> {
        host::run("/sbin/pfctl", &["-s", "info"])?;
        let device = host::Link::present(&self.0);
        let protected = host::Veil::walled(&self.0);
        let attended = host::Veil::attended(Span::new(&config.span)?.seat(), config.port);
        println!("instance {}", self.0.get());
        println!("  device: {}", present(device));
        println!("  wall: {}", present(protected));
        println!(
            "  resolver: {}",
            if attended { "answering" } else { "silent" }
        );
        let complete = device && attended && protected;
        if host::Veil::raised(&self.0) && !complete {
            println!("stranded: capture protection remains without a complete serving instance");
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
        let sources: Vec<_> = config
            .sources
            .split(',')
            .map(str::trim)
            .filter(|source| !source.is_empty())
            .map(str::to_string)
            .collect();
        let ground = host::Ground {
            local: config.local,
            port: config.port,
            claim,
            span: Span::new(&config.span)?,
            bare,
            sources: &sources,
            transit: config.transit,
        };
        host::establish(&self.0, &ground)?;
        println!("up: native macOS capture protection raised");
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn lower(&self) -> Result<ExitCode, Error> {
        service::stop(&self.0)?;
        host::reclaim(&self.0)?;
        println!("down: owned DNS and PF protection removed");
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
}

fn present(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}
