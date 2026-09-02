#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
mod node;
mod watch;

use dynet_api::outbound::PROBE;
use node::Aim;

use dynet_core::{Error, Instance};
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
    #[arg(long, global = true)]
    instance: Option<String>,
    #[arg(long, global = true)]
    port: Option<u16>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Up {
        #[arg(long, default_value = dynet_api::host::PREFIX)]
        claim: String,
        #[arg(long)]
        under: Option<String>,
    },
    Down,
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
    Forward {
        #[arg(long)]
        subscription: PathBuf,
        #[arg(long)]
        clusters: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long)]
        claim: String,
        #[arg(long, default_value = "80")]
        ports: String,
        #[arg(long, default_value = "1.1.1.1")]
        upstream: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        holds: String,
        #[arg(long, default_value = "30")]
        seconds: u64,
    },
}

#[derive(Debug, Cascade)]
struct Config {
    instance: String,
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instance: "dynet0".to_string(),
            port: 15353,
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
    let config = Config::resolve_with(
        None,
        ConfigPartial {
            instance: cli.instance,
            port: cli.port,
        },
    )
    .map_err(|error| Error::new(error.to_string()))?;
    let instance = Instance::new(&config.instance)?;
    match cli.command {
        Command::Doctor => doctor(&instance),
        Command::Up { claim, under } => raise(&instance, config.port, &claim, under.as_deref()),
        Command::Down => lower(&instance),
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
        Command::Forward {
            subscription,
            clusters,
            cluster,
            claim: _,
            ports,
            upstream,
            name,
            holds,
            seconds,
        } => watch::forward(
            &instance,
            &watch::Errand {
                subscription,
                clusters,
                cluster,
                ports,
                upstream,
                holds,
                port: config.port,
                seconds,
                name,
            },
        ),
    }
}

fn doctor(instance: &Instance) -> Result<ExitCode, Error> {
    let shape = host::survey()?;
    println!("resolver: {}", shape.explain());
    println!(
        "instance {}: priority {}",
        instance.get(),
        instance.priority()
    );
    println!("  device: {}", present(host::Link::present(instance)));
    println!("  veil: {}", present(host::Veil::raised(instance)));
    println!("  rule: {}", present(host::Route::declared(instance)));
    let registry = host::Route::registry(instance);
    println!("  {}: {:?}", registry.display(), host::held(&registry)?);
    let stray = host::sweep(instance);
    println!("strays: devices {:?} tables {:?}", stray.links, stray.veils);
    Ok(
        match shape.ownable() && stray.links.is_empty() && stray.veils.is_empty() {
            true => ExitCode::SUCCESS,
            false => ExitCode::from(1),
        },
    )
}

fn raise(
    instance: &Instance,
    port: u16,
    claim: &str,
    under: Option<&str>,
) -> Result<ExitCode, Error> {
    let standing = host::establish(instance, port, claim, under)?;
    println!(
        "up: reclaimed={} veiled={}",
        standing.cleared.any(),
        standing.veiled
    );
    Ok(ExitCode::SUCCESS)
}

fn lower(instance: &Instance) -> Result<ExitCode, Error> {
    let cleared = host::reclaim(instance)?;
    println!(
        "down: veil={} route={} link={}",
        cleared.veil, cleared.route, cleared.link
    );
    Ok(ExitCode::SUCCESS)
}

fn present(held: bool) -> &'static str {
    match held {
        true => "present",
        false => "absent",
    }
}
