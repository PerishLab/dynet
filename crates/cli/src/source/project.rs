use dynet_api::control::{Payload, Private, Session};
use dynet_api::host::State;
use dynet_api::project::{Recipient, derive, render, serve};
use dynet_core::{Error, Instance};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(clap::Args)]
pub struct Serve {
    #[arg(long)]
    bootstrap: PathBuf,
    #[arg(long, default_value = "127.0.0.1:18780")]
    listen: SocketAddr,
    #[arg(long)]
    user: bool,
}

#[derive(clap::Args)]
pub struct Url {
    #[arg(long)]
    input: PathBuf,
    #[arg(long = "base-url")]
    origin: String,
    #[arg(long)]
    recipient: Option<String>,
    #[arg(long)]
    at: Option<u64>,
    #[arg(long)]
    grace: bool,
}

#[derive(clap::Args)]
pub struct Render {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    at: Option<u64>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    Serve(Serve),
    Url(Url),
    Render(Render),
}

pub fn run(command: Command, instance: &Instance, state: &State) -> Result<ExitCode, Error> {
    match command {
        Command::Serve(args) => hosting(&args, instance, state),
        Command::Url(args) => address(&args),
        Command::Render(args) => dump(&args),
    }
}

fn hosting(args: &Serve, instance: &Instance, state: &State) -> Result<ExitCode, Error> {
    if !args.listen.ip().is_loopback() {
        return Err(Error::new(
            "dynet project serve requires a loopback --listen address",
        ));
    }
    let private = if args.user {
        Private::current()
    } else {
        Private::machine()
    };
    let session = Session::open(&args.bootstrap, instance, state, private)?;
    let listener = TcpListener::bind(args.listen)
        .map_err(|error| Error::new(format!("cannot bind {}: {error}", args.listen)))?;
    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop));
        let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop));
    }
    serve(listener, session, stop)?;
    Ok(ExitCode::SUCCESS)
}

fn address(args: &Url) -> Result<ExitCode, Error> {
    let payload = Payload::check(&super::load(&args.input)?)?;
    let spec = payload
        .projection
        .as_ref()
        .ok_or_else(|| Error::new("payload carries no projection section"))?;
    let slot = args.at.unwrap_or_else(clock) / spec.epoch();
    let base = args.origin.trim_end_matches('/');

    let chosen: Vec<&Recipient> =
        match args.recipient.as_deref() {
            Some(name) => vec![spec.recipient(name).ok_or_else(|| {
                Error::new(format!("projection recipient {name} is not declared"))
            })?],
            None => spec.recipients().iter().collect(),
        };

    for item in chosen {
        emit(
            item,
            base,
            slot,
            (args.recipient.is_some(), args.grace, spec.grace()),
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn emit(item: &Recipient, base: &str, slot: u64, mode: (bool, bool, u64)) {
    let token = derive(item.seed(), slot);
    if mode.0 && !mode.1 {
        println!("{base}/{token}");
        return;
    }
    println!("{}\tcurrent\t{base}/{token}", item.name());
    if !mode.1 {
        return;
    }
    for offset in 1..=mode.2 {
        if let Some(prev) = slot.checked_sub(offset) {
            let earlier = derive(item.seed(), prev);
            println!("{}\tgrace-{offset}\t{base}/{earlier}", item.name());
        }
    }
}

fn dump(args: &Render) -> Result<ExitCode, Error> {
    let payload = Payload::check(&super::load(&args.input)?)?;
    let yaml = render(
        &payload.loaded,
        payload.projection.as_ref(),
        args.at.unwrap_or_else(clock),
    )?;
    print!("{yaml}");
    Ok(ExitCode::SUCCESS)
}

fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
