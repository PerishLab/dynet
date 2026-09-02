#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
mod node;

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
    Up,
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
        Command::Up => raise(&instance, config.port),
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
        } => node::spread(&subscription, &cluster, count),
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

fn raise(instance: &Instance, port: u16) -> Result<ExitCode, Error> {
    let standing = host::establish(instance, port)?;
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
