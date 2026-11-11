mod clock;
#[cfg(test)]
mod tests;

use clock::Clock;
use dynet_core::{Error, Instance};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::time::Duration;

const CAPACITY: usize = 256;
const LIMIT: usize = 512;

pub(crate) struct Emit {
    queue: Option<Queue>,
}

struct Queue {
    sender: SyncSender<String>,
    lost: Arc<AtomicU64>,
    until: Arc<AtomicU64>,
    sequence: AtomicU64,
    clock: Arc<Clock>,
    stopped: Arc<AtomicBool>,
}

impl Emit {
    pub(crate) fn open(path: Option<&Path>, instance: &Instance) -> Result<Self, Error> {
        match path {
            None => Ok(Self { queue: None }),
            Some(path) => {
                Clock::private(path, true)?;
                Self::buffered(path.to_path_buf(), instance.get(), std::io::stdout())
            }
        }
    }

    fn buffered(
        path: PathBuf,
        label: &str,
        writer: impl Write + Send + 'static,
    ) -> Result<Self, Error> {
        let clock = Arc::new(Clock::open()?);
        let until = Arc::new(AtomicU64::new(clock.deadline(&path)));
        let lost = Arc::new(AtomicU64::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = sync_channel::<String>(CAPACITY);
        let queue = Queue {
            sender,
            lost: Arc::clone(&lost),
            until: Arc::clone(&until),
            sequence: AtomicU64::new(0),
            clock: Arc::clone(&clock),
            stopped: Arc::clone(&stopped),
        };
        let reader = Arc::clone(&clock);
        std::thread::Builder::new()
            .name("dynet-mode".into())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    until.store(reader.deadline(&path), Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(250));
                }
            })
            .map_err(|error| Error::new(error.to_string()))?;
        let label = label.to_string();
        std::thread::Builder::new()
            .name("dynet-record".into())
            .spawn(move || {
                Record {
                    writer,
                    lost,
                    clock,
                    label,
                    reported: u64::MAX,
                }
                .drain(receiver);
            })
            .map_err(|error| Error::new(error.to_string()))?;
        Ok(Self { queue: Some(queue) })
    }

    pub(crate) fn tell(&self, label: &str, line: &str) {
        let Some(queue) = &self.queue else {
            println!("  {line}");
            return;
        };
        let now = queue.clock.now();
        let detail = now < queue.until.load(Ordering::Relaxed);
        let class = classification(line);
        if !detail && class == "routine" {
            return;
        }
        let sequence = increment(&queue.sequence);
        let clean: String = line
            .chars()
            .take(LIMIT)
            .flat_map(char::escape_default)
            .take(LIMIT)
            .collect();
        let truncated = clean.len() >= LIMIT || line.chars().count() > LIMIT;
        let message = format!(
            "diag instance={label} pid={} seq={sequence} uptime={now} detail={detail} class={class} truncated={} {}\n",
            std::process::id(),
            truncated,
            clean
        );
        match queue.sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                increment(&queue.lost);
            }
        }
    }
}

impl Drop for Emit {
    fn drop(&mut self) {
        if let Some(queue) = &self.queue {
            queue.stopped.store(true, Ordering::Relaxed);
        }
    }
}

fn increment(value: &AtomicU64) -> u64 {
    value
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(1))
        })
        .unwrap_or_else(|current| current)
        .saturating_add(1)
}

fn classification(line: &str) -> &'static str {
    let fault = [
        "refused",
        "failed",
        "fault",
        "unavailable",
        "no node",
        "full",
        "configuration",
    ];
    if fault.iter().any(|word| line.contains(word)) {
        return "fault";
    }
    let routine = [
        " gave ",
        "left to the upstream",
        "answered none because this carries it",
        " becomes ",
        " names ",
        " aim=",
        " leaves directly on ",
        " ushered to ",
        " opened ",
    ];
    if routine.iter().any(|word| line.contains(word)) {
        "routine"
    } else {
        "unclassified"
    }
}

struct Record<W> {
    writer: W,
    lost: Arc<AtomicU64>,
    clock: Arc<Clock>,
    label: String,
    reported: u64,
}

impl<W: Write> Record<W> {
    fn drain(mut self, receiver: std::sync::mpsc::Receiver<String>) {
        loop {
            match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(line) => self.line(&line),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => self.report(),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn report(&mut self) {
        let total = self.lost.load(Ordering::Relaxed);
        if total == self.reported {
            return;
        }
        let report = format!(
            "diag instance={} pid={} lost={total} uptime={}\n",
            self.label,
            std::process::id(),
            self.clock.now()
        );
        if self.writer.write_all(report.as_bytes()).is_ok() {
            self.reported = total;
        }
    }

    fn line(&mut self, line: &str) {
        self.report();
        if self.writer.write_all(line.as_bytes()).is_err() {
            increment(&self.lost);
        }
    }
}
