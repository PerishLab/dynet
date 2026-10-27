use dynet_api::outbound::{Endpoint, PROBE, Roster, Run, Tunnel};
use dynet_api::{Entry, Loaded, resolver};
use dynet_core::{Cluster, Error, Form};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const LOOSE: u32 = 0;

pub struct Nodes<'a>(pub &'a Loaded);

pub fn lookup(entries: &[Entry], label: &str, name: &str) -> Result<Vec<resolver::Answer>, Error> {
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

pub fn board(entries: &[Entry], label: &str, host: &str, port: u16) -> Result<Tunnel, Error> {
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

pub fn visit(entries: &[Entry], label: &str) -> Result<Visit, Error> {
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

pub fn report(run: &Run, count: usize) -> ExitCode {
    println!("{} through cluster {}", PROBE, run.cluster().get());
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

impl Nodes<'_> {
    #[locus::trace(with = plumb::observation::view())]
    pub fn reach(&self, cluster: &str, count: usize) -> Result<ExitCode, Error> {
        let loaded = self.0;
        let entries = loaded.entries();
        let wanted = self.pooled(cluster)?;
        let labels: Vec<&str> = wanted
            .nodes()
            .iter()
            .map(|node| node.label().get())
            .take(count)
            .collect();
        let mut failures = 0;
        for (index, label) in labels.iter().enumerate() {
            let began = Instant::now();
            match visit(entries, label) {
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

    pub fn probe(&self, cluster: &str, name: &str, count: usize) -> Result<ExitCode, Error> {
        let loaded = self.0;
        let entries = loaded.entries();
        let wanted = self.pooled(cluster)?;
        for (index, node) in wanted.nodes().iter().take(count).enumerate() {
            match lookup(entries, node.label().get(), name) {
                Ok(found) => println!("{cluster}[{index}]: {found:?}"),
                Err(error) => println!("{cluster}[{index}]: refused: {error}"),
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    pub fn spread(&self, cluster: &str, aim: &Aim) -> Result<ExitCode, Error> {
        let loaded = self.0;
        let wanted = self.pooled(cluster)?;
        let roster = Roster::aimed(loaded.entries(), &aim.target, LOOSE);
        let run = roster.drive(wanted, aim.count)?;
        Ok(report(&run, aim.count))
    }

    fn pooled(&self, cluster: &str) -> Result<&Cluster, Error> {
        let loaded = self.0;
        let found = loaded
            .manifest()
            .clusters()
            .iter()
            .find(|item| item.name().get() == cluster)
            .ok_or_else(|| Error::new(format!("the manifest declares no cluster {cluster}")))?;
        match found.form() {
            Form::Pool(_) => Ok(found),
            Form::Alias(_) | Form::Via(..) => Err(Error::new(format!(
                "cluster {cluster} is not a pool; name the pool it leads to"
            ))),
        }
    }
}
