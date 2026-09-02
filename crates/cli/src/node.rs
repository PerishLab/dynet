use dynet_api::outbound::{Endpoint, Tunnel};
use dynet_api::{catalog, resolver, spread as pool, subscription};
use dynet_core::{Domain, Error, Name, Router, Rule, Subject, Table};
use std::path::PathBuf;
use std::process::ExitCode;

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
    )?;
    Tunnel::open(&endpoint, host, port)
}

pub fn visit(entries: &[subscription::Entry], label: &str) -> Result<String, Error> {
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

pub fn spread(path: &PathBuf, cluster: &str, count: usize) -> Result<ExitCode, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let entries = subscription::read(&text)?;
    let held = catalog::build(&entries)?;
    let wanted = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == cluster)
        .ok_or_else(|| Error::new(format!("no cluster named {cluster}")))?;
    let name = Domain::new(pool::PROBE)?;
    let router = Router::new(table(&name, wanted.name())?);
    let roster = pool::Roster::new(&entries);
    let run = roster.drive(wanted, &router.asked(&name), count)?;
    Ok(report(&run, count))
}

pub fn table(name: &Domain, cluster: &Name) -> Result<Table, Error> {
    let rule = Rule::new(Subject::Exact(name.clone()), cluster.clone());
    Ok(Table::new(vec![rule], Name::new("direct")?))
}

pub fn report(run: &pool::Run, count: usize) -> ExitCode {
    let decision = run.decision();
    println!(
        "rule {} -> cluster {} on ground {:?}",
        pool::PROBE,
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
