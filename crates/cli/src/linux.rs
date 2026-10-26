use crate::Config;
use dynet_api::host;
use dynet_core::{Error, Instance, Span};
use std::process::ExitCode;

pub(crate) struct Boundary(pub(crate) Instance);

impl Boundary {
    #[locus::trace(with = plumb::observation::view())]
    pub(crate) fn doctor(&self, config: &Config) -> Result<ExitCode, Error> {
        let shape = host::survey()?;
        println!("resolver: {}", shape.explain());
        println!("instance {}: priority {}", self.0.get(), self.0.priority());
        println!("  device: {}", present(host::Link::present(&self.0)));
        println!("  veil: {}", present(host::Veil::raised(&self.0)));
        println!("  rule: {}", present(host::Route::declared(&self.0)));
        let registry = host::Route::registry(&self.0);
        println!("  {}: {:?}", registry.display(), host::held(&registry)?);
        let stray = host::sweep(&self.0);
        println!("strays: devices {:?} tables {:?}", stray.links, stray.veils);
        let seat = Span::new(&config.span)?.seat();
        let held = host::Veil::attended(seat, config.port);
        println!("  resolver: {}", attending(held));
        if !held && host::Route::declared(&self.0) {
            println!(
                "stranded: the rules are installed and nothing answers behind them, so a caller meets a hole"
            );
            return Ok(ExitCode::from(1));
        }
        Ok(
            match shape.ownable() && stray.links.is_empty() && stray.veils.is_empty() {
                true => ExitCode::SUCCESS,
                false => ExitCode::from(1),
            },
        )
    }

    pub(crate) fn raise(
        &self,
        config: &Config,
        claim: &str,
        bare: bool,
    ) -> Result<ExitCode, Error> {
        let sources = scoped(&config.sources);
        let ground = host::Ground {
            port: config.port,
            claim,
            span: Span::new(&config.span)?,
            bare,
            sources: &sources,
            transit: config.transit,
        };
        let standing = host::establish(&self.0, &ground)?;
        println!(
            "up: reclaimed={} veiled={} capturing={} {}",
            standing.cleared.any(),
            standing.veiled,
            spoken(&sources),
            steered(config.transit, &sources)
        );
        Ok(ExitCode::SUCCESS)
    }

    pub(crate) fn lower(&self) -> Result<ExitCode, Error> {
        let cleared = host::reclaim(&self.0)?;
        println!(
            "down: veil={} route={} link={}",
            cleared.veil, cleared.route, cleared.link
        );
        Ok(ExitCode::SUCCESS)
    }

    #[locus::trace(with = plumb::observation::view())]
    pub(crate) fn reading(&self) -> Result<ExitCode, Error> {
        let path = dynet_api::inbound::standing::path(&self.0);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            Error::new(format!(
                "cannot read {}: {error}; the service writes it every sweep while it is diverting",
                path.display()
            ))
        })?;
        for line in text.lines().filter(|line| *line != host::MARKER) {
            println!("{line}");
        }
        Ok(ExitCode::SUCCESS)
    }
}

fn scoped(told: &str) -> Vec<String> {
    told.split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_string)
        .collect()
}

fn spoken(sources: &[String]) -> String {
    match sources.is_empty() {
        true => "this host".to_string(),
        false => format!("this host and {}", sources.join(", ")),
    }
}

fn attending(held: bool) -> &'static str {
    match held {
        true => "answering",
        false => "silent",
    }
}

fn steered(transit: u16, sources: &[String]) -> String {
    match transit == 0 || sources.is_empty() {
        true => "by the device alone".to_string(),
        false => format!("with forwarded streams ushered on :{transit} ahead of the device"),
    }
}

fn present(held: bool) -> &'static str {
    match held {
        true => "present",
        false => "absent",
    }
}
