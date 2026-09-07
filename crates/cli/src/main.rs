#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
mod node;
mod watch;

use dynet_api::outbound::PROBE;

const WHOLE: &str = "0.0.0.0/0";
use node::Aim;

use dynet_core::{Error, Instance, Span};
use plumb::config::Cascade;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "dynet",
    version = plumb::version!("DYNET"),
    about = "Dynet command-line boundary"
)]
struct Cli {
    #[command(flatten)]
    told: ConfigArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Up {
        #[arg(long, default_value = WHOLE)]
        claim: String,
        #[arg(long)]
        bare: bool,
    },
    Down,
    Standing,
    Divert(watch::Errand),
    Resolve {
        #[arg(long)]
        subscription: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "6")]
        count: usize,
    },
    Reach {
        #[arg(long)]
        subscription: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long, default_value = "8")]
        count: usize,
    },
    Spread {
        #[arg(long)]
        subscription: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long, default_value = "16")]
        count: usize,
        #[arg(long, default_value = PROBE)]
        target: String,
    },
    Exit {
        #[arg(long)]
        seat: String,
        #[arg(long)]
        secret: String,
        #[arg(long, default_value = "api.ipify.org")]
        target: String,
    },
}

#[derive(Debug, Cascade)]
struct Config {
    #[cascade(arg)]
    instance: String,
    #[cascade(arg)]
    port: u16,
    span: String,
    sources: String,
    transit: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instance: "dynet0".to_string(),
            port: 15353,
            span: host::PREFIX.to_string(),
            sources: String::new(),
            transit: 0,
        }
    }
}

fn main() -> ExitCode {
    debug_assert_eq!(dynet_api::VERSION, env!("CARGO_PKG_VERSION"));
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("refused: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, Error> {
    let config = Config::resolve_with(None, cli.told.partial())
        .map_err(|error| Error::new(error.to_string()))?;
    let instance = Instance::new(&config.instance)?;
    match cli.command {
        Command::Doctor => Boundary(instance).doctor(),
        Command::Up { claim, bare } => Boundary(instance).raise(&config, &claim, bare),
        Command::Down => Boundary(instance).lower(),
        Command::Standing => Boundary(instance).reading(),
        Command::Divert(errand) => watch::divert(&instance, &staged(&config, &errand)?),
        Command::Resolve {
            subscription,
            cluster,
            name,
            count,
        } => node::probe(&subscription, &cluster, &name, count),
        Command::Reach {
            subscription,
            cluster,
            count,
        } => node::reach(&subscription, &cluster, count),
        Command::Spread {
            subscription,
            cluster,
            count,
            target,
        } => node::spread(&subscription, &cluster, &Aim { count, target }),
        Command::Exit {
            seat,
            secret,
            target,
        } => node::exit(&seat, &secret, &target),
    }
}

fn staged<'a>(config: &Config, errand: &'a watch::Errand) -> Result<watch::Stage<'a>, Error> {
    Ok(watch::Stage {
        errand,
        port: config.port,
        span: Span::new(&config.span)?,
        transit: config.transit,
    })
}

struct Boundary(Instance);

impl Boundary {
    fn doctor(&self) -> Result<ExitCode, Error> {
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
        Ok(
            match shape.ownable() && stray.links.is_empty() && stray.veils.is_empty() {
                true => ExitCode::SUCCESS,
                false => ExitCode::from(1),
            },
        )
    }

    fn raise(&self, config: &Config, claim: &str, bare: bool) -> Result<ExitCode, Error> {
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

    fn lower(&self) -> Result<ExitCode, Error> {
        let cleared = host::reclaim(&self.0)?;
        println!(
            "down: veil={} route={} link={}",
            cleared.veil, cleared.route, cleared.link
        );
        Ok(ExitCode::SUCCESS)
    }

    fn reading(&self) -> Result<ExitCode, Error> {
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
