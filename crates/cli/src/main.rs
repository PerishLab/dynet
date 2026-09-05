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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instance: "dynet0".to_string(),
            port: 15353,
            span: host::PREFIX.to_string(),
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
        Command::Doctor => doctor(&instance),
        Command::Up { claim, bare } => raise(&instance, &config, &claim, bare),
        Command::Down => lower(&instance),
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
    })
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

fn raise(instance: &Instance, config: &Config, claim: &str, bare: bool) -> Result<ExitCode, Error> {
    let ground = host::Ground {
        port: config.port,
        claim,
        span: Span::new(&config.span)?,
        bare,
    };
    let standing = host::establish(instance, &ground)?;
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
