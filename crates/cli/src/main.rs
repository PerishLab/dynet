#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
mod carry;
mod flush;
mod node;
mod observation;
mod platform;
mod source;
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
    Identity {
        #[arg(long)]
        output: PathBuf,
    },
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
        bootstrap: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "6")]
        count: usize,
    },
    Reach {
        #[arg(long)]
        bootstrap: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long, default_value = "8")]
        count: usize,
    },
    Spread {
        #[arg(long)]
        bootstrap: PathBuf,
        #[arg(long)]
        cluster: String,
        #[arg(long, default_value = "16")]
        count: usize,
        #[arg(long, default_value = PROBE)]
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
    local: bool,
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
            local: true,
            sources: String::new(),
            transit: 0,
        }
    }
}

impl Config {
    fn validate(&self) -> Result<(), Error> {
        if self.local {
            return Ok(());
        }
        if !cfg!(target_os = "linux") {
            return Err(Error::new(
                "forwarded-only capture is offered on Linux alone",
            ));
        }
        if self
            .sources
            .split(',')
            .all(|source| source.trim().is_empty())
        {
            return Err(Error::new(
                "forwarded-only capture requires declared sources",
            ));
        }
        Ok(())
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
    if matches!(&cli.command, Command::Up { .. } | Command::Divert(_)) {
        config.validate()?;
    }
    let instance = Instance::new(&config.instance)?;
    let state = host::State::new(config.state.clone())?;
    #[cfg(target_os = "macos")]
    host::macos::configure(state.root())?;
    match cli.command {
        Command::Identity { output } => {
            let recipient =
                dynet_api::control::identity(&output, dynet_api::control::Private::machine())?;
            println!("{recipient}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => Boundary(instance).doctor(&config),
        Command::Up { claim, bare } => Boundary(instance).raise(&config, &claim, bare),
        Command::Down => Boundary(instance).lower(),
        Command::Standing => Boundary(instance).reading(&state),
        Command::Divert(errand) => watch::divert(&instance, &staged(&config, &errand, &state)?),
        Command::Carry(errand) => {
            let session = source::read(errand.path(), &instance, &state)?;
            carry::carry(&errand, source::loaded(&session)?)
        }
        Command::Flush(errand) => flush::flush(&instance, &errand, &state),
        #[cfg(any(windows, target_os = "macos"))]
        Command::Service(service) => Boundary(instance).service(&config, &service, &state),
        Command::Resolve {
            bootstrap,
            cluster,
            name,
            count,
        } => {
            let session = source::read(&bootstrap, &instance, &state)?;
            node::Nodes(source::loaded(&session)?).probe(&cluster, &name, count)
        }
        Command::Reach {
            bootstrap,
            cluster,
            count,
        } => {
            let session = source::read(&bootstrap, &instance, &state)?;
            node::Nodes(source::loaded(&session)?).reach(&cluster, count)
        }
        Command::Spread {
            bootstrap,
            cluster,
            count,
            target,
        } => {
            let session = source::read(&bootstrap, &instance, &state)?;
            node::Nodes(source::loaded(&session)?).spread(&cluster, &Aim { count, target })
        }
    }
}

fn staged<'a>(
    config: &'a Config,
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
        config,
        errand,
        state,
        port: config.port,
        span: Span::new(&config.span)?,
        transit: config.transit,
    })
}
