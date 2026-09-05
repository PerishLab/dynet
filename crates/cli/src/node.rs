use dynet_api::outbound::veil;
use dynet_api::outbound::{Endpoint, PROBE, Roster, Run, Tunnel};
use dynet_api::{catalog, resolver, subscription};
use dynet_core::{Domain, Error, Name, Router, Rule, Subject, Table};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const LOOSE: u32 = 0;

pub fn reach(path: &PathBuf, cluster: &str, count: usize) -> Result<ExitCode, Error> {
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
        let began = Instant::now();
        match visit(&entries, label) {
            Ok(seen) => println!(
                "{cluster}[{index}]: {} opened {}ms whole {}ms",
                seen.egress,
                seen.opened.as_millis(),
                seen.whole.as_millis()
            ),
            Err(error) => {
                failures += 1;
                println!(
                    "{cluster}[{index}]: refused after {}ms: {error}",
                    began.elapsed().as_millis()
                );
            }
        }
    }
    Ok(match failures {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    })
}

pub fn probe(path: &PathBuf, cluster: &str, name: &str, count: usize) -> Result<ExitCode, Error> {
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

pub fn lookup(
    entries: &[subscription::Entry],
    label: &str,
    name: &str,
) -> Result<Vec<resolver::Answer>, Error> {
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
    resolver::read(answer.get(2..).unwrap_or_default())
}

pub fn board(
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
        LOOSE,
    )?;
    Tunnel::open(&endpoint, host, port)
}

pub struct Visit {
    pub egress: String,
    pub opened: Duration,
    pub whole: Duration,
}

pub fn visit(entries: &[subscription::Entry], label: &str) -> Result<Visit, Error> {
    let began = Instant::now();
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
    let endpoint = Endpoint::new(host, port, uuid, LOOSE)?;
    let mut tunnel = Tunnel::open(&endpoint, "api.ipify.org", 80)?;
    let opened = began.elapsed();
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
    Ok(Visit {
        egress: text
            .rsplit("\r\n\r\n")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
        opened,
        whole: began.elapsed(),
    })
}

pub struct Aim {
    pub count: usize,
    pub target: String,
}

pub fn spread(path: &PathBuf, cluster: &str, aim: &Aim) -> Result<ExitCode, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let entries = subscription::read(&text)?;
    let held = catalog::build(&entries)?;
    let wanted = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == cluster)
        .ok_or_else(|| Error::new(format!("no cluster named {cluster}")))?;
    let name = Domain::new(PROBE)?;
    let router = Router::new(table(&name, wanted.name())?);
    let roster = Roster::aimed(&entries, &aim.target, LOOSE);
    let run = roster.drive(wanted, &router.asked(&name), aim.count)?;
    Ok(report(&run, aim.count))
}

pub fn table(name: &Domain, cluster: &Name) -> Result<Table, Error> {
    let rule = Rule::new(Subject::Exact(name.clone()), cluster.clone());
    Ok(Table::new(vec![rule], Name::new("direct")?))
}

pub fn report(run: &Run, count: usize) -> ExitCode {
    let decision = run.decision();
    println!(
        "rule {} -> cluster {} on ground {:?}",
        PROBE,
        decision.cluster().get(),
        decision.ground()
    );
    for trial in run.trials() {
        println!(
            "  {:<28} {:<24} {:?} {}",
            trial.label().get(),
            trial.egress().unwrap_or("-"),
            trial.verdict(),
            trial.note().unwrap_or_default()
        );
    }
    let egresses = run.egresses();
    println!(
        "nodes {} of {} requests, answered {}, faulted {}, distinct egresses {}",
        run.borne().len(),
        count,
        run.answered(),
        run.faulted(),
        egresses.len()
    );
    match run.answered() == count && egresses.len() > 1 {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    }
}

pub fn exit(seat: &str, secret: &str, target: &str) -> Result<ExitCode, Error> {
    let drawn = unbase(secret).ok_or_else(|| Error::new("the key is not base sixty four"))?;
    let mut key = [0u8; 16];
    if drawn.len() != key.len() {
        return Err(Error::new(format!(
            "the key carries {} bytes, not sixteen",
            drawn.len()
        )));
    }
    key.copy_from_slice(&drawn);
    let mut stream = std::net::TcpStream::connect(seat)
        .map_err(|error| Error::new(format!("cannot reach {seat}: {error}")))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(15)))
        .map_err(|error| Error::new(format!("cannot bound the session: {error}")))?;
    let (mut cloak, mut shroud) = veil(key)?;
    let asking =
        format!("GET /?format=text HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n");
    cloak.greet(&mut stream, (target, 80), asking.as_bytes())?;
    let mut answer = Vec::new();
    while let Some(part) = shroud.receive(&mut stream)? {
        answer.extend_from_slice(&part);
        if answer.len() > 4096 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&answer);
    let egress = text.rsplit("\r\n\r\n").next().unwrap_or_default().trim();
    println!("{seat} speaks from {egress}");
    Ok(match text.starts_with("HTTP/1.1 200") {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    })
}

pub fn unbase(text: &str) -> Option<Vec<u8>> {
    const CODE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut held = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for byte in text.bytes().filter(|item| *item != b'=') {
        let place = u32::try_from(CODE.iter().position(|item| *item == byte)?).ok()?;
        held = (held << 6) | place;
        bits += 6;
        if bits < 8 {
            continue;
        }
        bits -= 8;
        out.push(u8::try_from((held >> bits) & 0xff).ok()?);
    }
    Some(out)
}
