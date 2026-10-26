use dynet_api::outbound::{Exit, Front, Listener, Roster, Speaker, veil};
use dynet_api::{Entry, manifest};
use dynet_core::{Error, Label};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const LOOSE: u32 = 0;
const CEILING: usize = 4096;

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    node: String,
    #[arg(long)]
    exit: Option<String>,
    #[arg(long, default_value = "api.ipify.org")]
    target: String,
    #[arg(long, default_value = "80")]
    port: u16,
    #[arg(long)]
    listen: Option<String>,
}

struct Plan {
    front: Front,
    exit: Option<Exit>,
}

#[locus::trace(with = plumb::observation::view())]
pub fn carry(errand: &Errand) -> Result<ExitCode, Error> {
    let loaded = manifest::read(&errand.manifest)?;
    let entries = loaded.entries();
    let plan = Plan {
        front: Front::read(
            found(entries, &errand.node)?,
            served(entries, &errand.node)?,
            LOOSE,
        )?,
        exit: match &errand.exit {
            Some(name) => Some(Roster::new(entries, LOOSE).exit(&Label::new(name.as_str())?)?),
            None => None,
        },
    };
    match &errand.listen {
        Some(seat) => relay(&plan, errand, seat),
        None => fetch(&plan, errand),
    }
}

fn found<'a>(entries: &'a [Entry], name: &str) -> Result<&'a Entry, Error> {
    entries
        .iter()
        .find(|item| item.field("name") == Some(name))
        .ok_or_else(|| Error::new(format!("the manifest declares no node {name}")))
}

fn served(entries: &[Entry], name: &str) -> Result<String, Error> {
    found(entries, name)?
        .field("server")
        .map(str::to_string)
        .ok_or_else(|| Error::new(format!("node {name} names no server")))
}

fn open(plan: &Plan, target: &str, port: u16) -> Result<(Speaker, Listener), Error> {
    let Some(exit) = &plan.exit else {
        let (egress, ingress) = plan.front.dial(target, port)?;
        return Ok((Speaker::Plain(egress), Listener::Plain(ingress)));
    };
    let (mut egress, ingress) = plan.front.dial(&exit.host, exit.port)?;
    let (mut cloak, shroud) = veil(exit.secret)?;
    cloak.greet(&mut egress, (target, port), &[])?;
    Ok((
        Speaker::Veiled(egress, Box::new(cloak)),
        Listener::Veiled(ingress, Box::new(shroud)),
    ))
}

fn fetch(plan: &Plan, errand: &Errand) -> Result<ExitCode, Error> {
    let began = Instant::now();
    let (mut speaker, mut listener) = open(plan, &errand.target, errand.port)?;
    let opened = began.elapsed();
    let asking = format!(
        "GET /?format=text HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        errand.target
    );
    speaker.send(asking.as_bytes())?;
    let mut answer = Vec::new();
    while answer.len() < CEILING {
        match listener.receive()? {
            Some(part) => answer.extend_from_slice(&part),
            None => break,
        }
    }
    let text = String::from_utf8_lossy(&answer);
    let egress = text.rsplit("\r\n\r\n").next().unwrap_or_default().trim();
    println!(
        "{} speaks from {egress} opened {}ms whole {}ms",
        spoken(errand),
        opened.as_millis(),
        began.elapsed().as_millis()
    );
    Ok(match text.starts_with("HTTP/1.1 200") {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    })
}

fn relay(plan: &Plan, errand: &Errand, seat: &str) -> Result<ExitCode, Error> {
    let listener = TcpListener::bind(seat)
        .map_err(|error| Error::new(format!("cannot listen on {seat}: {error}")))?;
    println!(
        "{} carries {seat} to {}:{}",
        spoken(errand),
        errand.target,
        errand.port
    );
    let (caller, _) = listener
        .accept()
        .map_err(|error| Error::new(format!("cannot accept a caller: {error}")))?;
    let (mut speaker, mut node) = open(plan, &errand.target, errand.port)?;
    let mut reading = caller
        .try_clone()
        .map_err(|error| Error::new(format!("cannot halve the caller: {error}")))?;
    let back = std::thread::spawn(move || answer(&mut node, caller));
    let mut room = vec![0u8; 16384];
    loop {
        let size = reading.read(&mut room).unwrap_or(0);
        if size == 0 {
            let _ = speaker.done();
            break;
        }
        speaker.send(&room[..size])?;
    }
    let carried = back.join().unwrap_or(0);
    println!("carried {carried} bytes back");
    Ok(ExitCode::SUCCESS)
}

fn answer(node: &mut Listener, mut caller: TcpStream) -> usize {
    let mut carried = 0;
    while let Ok(Some(part)) = node.receive() {
        carried += part.len();
        if caller.write_all(&part).is_err() {
            break;
        }
    }
    let _ = caller.shutdown(std::net::Shutdown::Write);
    carried
}

fn spoken(errand: &Errand) -> String {
    match &errand.exit {
        Some(exit) => format!("{} through {}", exit, errand.node),
        None => errand.node.clone(),
    }
}
