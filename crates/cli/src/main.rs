#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
mod carry;
mod flush;
mod node;
mod observation;
mod platform;
mod watch;

use platform::Boundary;
#[cfg(any(windows, target_os = "macos"))]
use platform::Service;

use dynet_api::outbound::PROBE;

const WHOLE: &str = "0.0.0.0/0";
#[cfg(target_os = "linux")]
const PORT: u16 = 15353;
#[cfg(any(windows, target_os = "macos"))]
const PORT: u16 = 53;
use node::Aim;

use dynet_core::{Error, Instance, Span};
use plumb::config::Cascade;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "dynet",
    version = plumb::version!("DYNET"),
    about = "Dynet command-line boundary",
    after_help = "Short-command observation is disabled by default. Set [locus] enabled = true in config.toml under ~/.dynet on Unix or the LocalAppData known folder under dynet on Windows. Buffers use state/locus there. endpoint overrides LOCUS_API, then http://127.0.0.1:43308. Observation failures preserve output and exit status. Continuous divert initializes no observation engine; divert --unit observes only unit generation. Local state uses one --state or DYNET_STATE root, with runtime snapshots and flush communication in its run directory. Windows defaults to the ProgramData known folder under dynet; the service registration captures the resolved absolute root. macOS machine state defaults to /Library/Application Support/Dynet with runtime in run; service registration requires root-owned paths. Linux defaults remain /var/lib/dynet and /run/dynet."
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
    Carry(carry::Errand),
    Flush(flush::Errand),
    #[cfg(any(windows, target_os = "macos"))]
    #[command(subcommand)]
    Service(Service),
    Resolve {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "6")]
        count: usize,
    },
    Reach {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long, default_value = "8")]
        count: usize,
    },
    Spread {
        #[arg(long)]
        manifest: PathBuf,
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
pub(crate) struct Config {
    #[cascade(arg)]
    instance: String,
    #[cascade(arg)]
    port: u16,
    #[cascade(arg)]
    span: String,
    #[cascade(arg)]
    state: Option<PathBuf>,
    sources: String,
    transit: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instance: "dynet0".to_string(),
            port: PORT,
            span: host::PREFIX.to_string(),
            state: None,
            sources: String::new(),
            transit: 0,
        }
    }
}

fn main() -> ExitCode {
    debug_assert_eq!(dynet_api::VERSION, env!("CARGO_PKG_VERSION"));
    if let Err(error) = plumb::identity!("DYNET") {
        eprintln!("refused: {error}");
        return ExitCode::from(2);
    }
    let cli = Cli::parse();
    let observer = if matches!(&cli.command, Command::Divert(errand) if !errand.unit) {
        None
    } else {
        observation::start(observation::name(&cli.command))
    };
    let code = match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("refused: {error}");
            ExitCode::from(2)
        }
    };
    if let Some(observer) = observer {
        observer.finish(if code == ExitCode::SUCCESS {
            0
        } else if code == ExitCode::from(1) {
            1
        } else {
            2
        });
    }
    code
}

#[locus::trace(with = plumb::observation::view())]
fn run(cli: Cli) -> Result<ExitCode, Error> {
    let config =
        Config::layered(None, cli.told.partial()).map_err(|error| Error::new(error.to_string()))?;
    let instance = Instance::new(&config.instance)?;
    let state = host::State::new(config.state.clone())?;
    #[cfg(target_os = "macos")]
    host::macos::configure(state.root())?;
    match cli.command {
        Command::Doctor => Boundary(instance).doctor(&config),
        Command::Up { claim, bare } => Boundary(instance).raise(&config, &claim, bare),
        Command::Down => Boundary(instance).lower(),
        Command::Standing => Boundary(instance).reading(&state),
        Command::Divert(errand) => watch::divert(&instance, &staged(&config, &errand, &state)?),
        Command::Carry(errand) => carry::carry(&errand),
        Command::Flush(errand) => flush::flush(&instance, &errand, &state),
        #[cfg(any(windows, target_os = "macos"))]
        Command::Service(service) => Boundary(instance).service(&config, &service, &state),
        Command::Resolve {
            manifest,
            cluster,
            name,
            count,
        } => node::probe(&manifest, &cluster, &name, count),
        Command::Reach {
            manifest,
            cluster,
            count,
        } => node::reach(&manifest, &cluster, count),
        Command::Spread {
            manifest,
            cluster,
            count,
            target,
        } => node::spread(&manifest, &cluster, &Aim { count, target }),
        Command::Exit {
            seat,
            secret,
            target,
        } => node::exit(&seat, &secret, &target),
    }
}

fn staged<'a>(
    config: &Config,
    errand: &'a watch::Errand,
    state: &'a host::State,
) -> Result<watch::Stage<'a>, Error> {
    #[cfg(target_os = "macos")]
    if !config.sources.is_empty() {
        return Err(Error::new(
            "macOS captures this host alone; forwarded sources refuse",
        ));
    }
    Ok(watch::Stage {
        errand,
        state,
        port: config.port,
        span: Span::new(&config.span)?,
        transit: config.transit,
    })
}
