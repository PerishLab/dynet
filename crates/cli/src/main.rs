#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use dynet_api::host;
use dynet_api::outbound::{Endpoint, Tunnel};
use dynet_api::{catalog, resolver, subscription};
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
        } => probe(&subscription, &cluster, &name, count),
        Command::Reach {
            subscription,
            cluster,
            count,
        } => reach(&subscription, &cluster, count),
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

fn reach(path: &PathBuf, cluster: &str, count: usize) -> Result<ExitCode, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let entries = subscription::read(&text)?;
    let held = catalog::build(&entries)?;
    let wanted = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == cluster)
        .ok_or_else(|| Error::new(format!("no cluster named {cluster}")))?;
    let labels: Vec<&str> = wanted
        .nodes()
        .iter()
        .map(|node| node.label().get())
        .take(count)
        .collect();
    let mut failures = 0;
    for (index, label) in labels.iter().enumerate() {
        match visit(&entries, label) {
            Ok(egress) => println!("{cluster}[{index}]: {egress}"),
            Err(error) => {
                failures += 1;
                println!("{cluster}[{index}]: refused: {error}");
            }
        }
    }
    Ok(match failures {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    })
}

fn probe(path: &PathBuf, cluster: &str, name: &str, count: usize) -> Result<ExitCode, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let entries = subscription::read(&text)?;
    let held = catalog::build(&entries)?;
    let wanted = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == cluster)
        .ok_or_else(|| Error::new(format!("no cluster named {cluster}")))?;
    for (index, node) in wanted.nodes().iter().take(count).enumerate() {
        match lookup(&entries, node.label().get(), name) {
            Ok(found) => println!("{cluster}[{index}]: {found:?}"),
            Err(error) => println!("{cluster}[{index}]: refused: {error}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn lookup(
    entries: &[subscription::Entry],
    label: &str,
    name: &str,
) -> Result<Vec<std::net::Ipv4Addr>, Error> {
    let mut tunnel = board(entries, label, "1.1.1.1", 53)?;
    tunnel.send(&resolver::ask(name, 0x2b2b)?)?;
    let mut answer = Vec::new();
    while let Some(part) = tunnel.receive()? {
        answer.extend_from_slice(&part);
        if answer.len() >= 2 {
            let want = usize::from(u16::from_be_bytes([answer[0], answer[1]]));
            if answer.len() >= want + 2 {
                break;
            }
        }
    }
    resolver::read(&answer)
}

fn board(
    entries: &[subscription::Entry],
    label: &str,
    host: &str,
    port: u16,
) -> Result<Tunnel, Error> {
    let entry = entries
        .iter()
        .find(|item| item.field("name") == Some(label))
        .ok_or_else(|| Error::new("no such node"))?;
    let endpoint = Endpoint::new(
        entry
            .field("server")
            .ok_or_else(|| Error::new("no server"))?,
        entry
            .field("port")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::new("no port"))?,
        entry
            .field("uuid")
            .ok_or_else(|| Error::new("no identity"))?,
    )?;
    Tunnel::open(&endpoint, host, port)
}

fn visit(entries: &[subscription::Entry], label: &str) -> Result<String, Error> {
    let entry = entries
        .iter()
        .find(|item| item.field("name") == Some(label))
        .ok_or_else(|| Error::new("no such node"))?;
    let host = entry
        .field("server")
        .ok_or_else(|| Error::new("no server"))?;
    let port: u16 = entry
        .field("port")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| Error::new("no port"))?;
    let uuid = entry
        .field("uuid")
        .ok_or_else(|| Error::new("no identity"))?;
    let endpoint = Endpoint::new(host, port, uuid)?;
    let mut tunnel = Tunnel::open(&endpoint, "api.ipify.org", 80)?;
    tunnel
        .send(b"GET /?format=text HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n")?;
    let mut answer = Vec::new();
    while let Some(part) = tunnel.receive()? {
        answer.extend_from_slice(&part);
        if answer.len() > 4096 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&answer);
    if !text.starts_with("HTTP/1.1 200") {
        return Err(Error::new(format!("no answer: {:.60}", text)));
    }
    Ok(text
        .rsplit("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string())
}

fn present(held: bool) -> &'static str {
    match held {
        true => "present",
        false => "absent",
    }
}
