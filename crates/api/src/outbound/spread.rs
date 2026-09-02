use super::{Endpoint, Tunnel};
use crate::subscription::Entry;
use dynet_core::{Cluster, Decision, Error, Fault, Label, Policy, Selector, Verdict};
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PROBE: &str = "api.ipify.org";
const ASK: &str = "GET /?format=text HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n";
const HALF: Duration = Duration::from_secs(600);
const FLOOR: f64 = 0.05;
const CEILING: usize = 4096;

enum Outcome {
    Answered(u16, String),
    Faulted(Fault, String),
}

#[derive(Clone, Debug)]
pub struct Trial {
    label: Label,
    verdict: Verdict,
    egress: Option<String>,
    note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Run {
    decision: Decision,
    trials: Vec<Trial>,
}

impl Trial {
    pub fn label(&self) -> &Label {
        &self.label
    }

    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    pub fn egress(&self) -> Option<&str> {
        self.egress.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

impl Run {
    pub fn decision(&self) -> &Decision {
        &self.decision
    }

    pub fn trials(&self) -> &[Trial] {
        &self.trials
    }

    pub fn egresses(&self) -> BTreeSet<&str> {
        self.trials.iter().filter_map(Trial::egress).collect()
    }

    pub fn borne(&self) -> BTreeSet<&str> {
        self.trials
            .iter()
            .map(|trial| trial.label().get())
            .collect()
    }

    pub fn answered(&self) -> usize {
        self.trials
            .iter()
            .filter(|trial| trial.verdict() == Verdict::Answered)
            .count()
    }

    pub fn faulted(&self) -> usize {
        self.trials.len() - self.answered()
    }
}

pub struct Roster<'a> {
    entries: &'a [Entry],
    target: String,
}

impl<'a> Roster<'a> {
    pub fn new(entries: &'a [Entry]) -> Self {
        Self::aimed(entries, PROBE)
    }

    pub fn aimed(entries: &'a [Entry], target: &str) -> Self {
        Self {
            entries,
            target: target.to_string(),
        }
    }

    pub fn drive(
        &self,
        cluster: &Cluster,
        decision: &Decision,
        count: usize,
    ) -> Result<Run, Error> {
        let policy = Policy::new(HALF, FLOOR)?;
        let selector = Mutex::new(Selector::new(cluster, policy, Instant::now()));
        let trials = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..count {
                scope.spawn(|| self.attempt(&selector, &trials));
            }
        });
        let mut trials = trials
            .into_inner()
            .map_err(|_| Error::new("a trial thread died holding the ledger"))?;
        trials.sort_by(|left, right| left.label().get().cmp(right.label().get()));
        Ok(Run {
            decision: decision.clone(),
            trials,
        })
    }

    fn attempt(&self, selector: &Mutex<Selector>, trials: &Mutex<Vec<Trial>>) {
        let Some(label) = pick(selector) else {
            return;
        };
        let (verdict, egress, note) = judge(self.visit(&label));
        if let Ok(mut held) = selector.lock() {
            held.observed(&label, verdict, Instant::now());
        }
        if let Ok(mut held) = trials.lock() {
            held.push(Trial {
                label,
                verdict,
                egress,
                note,
            });
        }
    }

    fn visit(&self, label: &Label) -> Outcome {
        match self.reach(label) {
            Err(outcome) => outcome,
            Ok(mut tunnel) => collect(&mut tunnel),
        }
    }

    fn reach(&self, label: &Label) -> Result<Tunnel, Outcome> {
        let endpoint = self
            .board(label)
            .map_err(|error| stumble(Fault::Reach, &error))?;
        let mut tunnel = Tunnel::dial(&endpoint).map_err(|error| stumble(Fault::Reach, &error))?;
        tunnel
            .board(&endpoint, &self.target, 80)
            .map_err(|error| stumble(Fault::Handshake, &error))?;
        tunnel
            .send(ASK.as_bytes())
            .map_err(|error| stumble(Fault::Severed, &error))?;
        Ok(tunnel)
    }

    pub fn posted(
        &self,
        label: &Label,
        book: &super::Book,
        upstream: &str,
    ) -> Result<Endpoint, Error> {
        let entry = self.seat(label)?;
        let host = entry
            .field("server")
            .ok_or_else(|| Error::new("no server"))?;
        Endpoint::new(
            book.find(host, upstream)?,
            port(entry)?,
            entry
                .field("uuid")
                .ok_or_else(|| Error::new("no identity"))?,
        )
    }

    pub fn exit(&self, label: &Label) -> Result<super::Exit, Error> {
        let entry = self.seat(label)?;
        let told = entry
            .field("secret")
            .ok_or_else(|| Error::new(format!("node {} carries no key", label.get())))?;
        let drawn =
            super::raw::unbase(told).ok_or_else(|| Error::new("a key is not base sixty four"))?;
        let mut secret = [0u8; 16];
        if drawn.len() != secret.len() {
            return Err(Error::new(format!(
                "node {} carries a key of {} bytes, not sixteen",
                label.get(),
                drawn.len()
            )));
        }
        secret.copy_from_slice(&drawn);
        Ok(super::Exit {
            host: entry
                .field("server")
                .ok_or_else(|| Error::new("no server"))?
                .to_string(),
            port: port(entry)?,
            secret,
        })
    }

    fn seat(&self, label: &Label) -> Result<&Entry, Error> {
        self.entries
            .iter()
            .find(|item| item.field("name") == Some(label.get()))
            .ok_or_else(|| Error::new(format!("no node named {}", label.get())))
    }

    pub fn board(&self, label: &Label) -> Result<Endpoint, Error> {
        let entry = self
            .entries
            .iter()
            .find(|item| item.field("name") == Some(label.get()))
            .ok_or_else(|| Error::new(format!("no node named {}", label.get())))?;
        let port = entry
            .field("port")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::new("a node carries no usable port"))?;
        Endpoint::new(
            entry
                .field("server")
                .ok_or_else(|| Error::new("no server"))?,
            port,
            entry
                .field("uuid")
                .ok_or_else(|| Error::new("no identity"))?,
        )
    }
}

fn port(entry: &Entry) -> Result<u16, Error> {
    entry
        .field("port")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| Error::new("a node carries no usable port"))
}

fn pick(selector: &Mutex<Selector>) -> Option<Label> {
    selector.lock().ok()?.choose(Instant::now())
}

fn judge(outcome: Outcome) -> (Verdict, Option<String>, Option<String>) {
    match outcome {
        Outcome::Answered(200, egress) => (Verdict::Answered, Some(egress), None),
        Outcome::Answered(status, egress) => (
            Verdict::Blocked,
            Some(egress),
            Some(format!("the destination answered {status}")),
        ),
        Outcome::Faulted(fault, note) => (Verdict::Faulted(fault), None, Some(note)),
    }
}

fn collect(tunnel: &mut Tunnel) -> Outcome {
    let mut answer = Vec::new();
    while answer.len() < CEILING {
        match tunnel.receive() {
            Ok(None) => break,
            Ok(Some(part)) => answer.extend_from_slice(&part),
            Err(error) => {
                let fault = tunnel.fault().unwrap_or(Fault::Severed);
                return stumble(fault, &error);
            }
        }
    }
    settle(&answer)
}

fn settle(answer: &[u8]) -> Outcome {
    let text = String::from_utf8_lossy(answer);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok());
    let Some(status) = status else {
        return Outcome::Faulted(Fault::Severed, "the answer carried no status".to_string());
    };
    let egress = text.rsplit("\r\n\r\n").next().unwrap_or_default().trim();
    Outcome::Answered(status, egress.to_string())
}

fn stumble(fault: Fault, error: &Error) -> Outcome {
    Outcome::Faulted(fault, error.to_string())
}
