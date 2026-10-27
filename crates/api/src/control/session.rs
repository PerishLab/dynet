use super::{Bootstrap, Cache, Candidate, Decoder, Payload, custody, refused, worker::Worker};
use crate::host::State;
use dynet_core::{Error, Instance};
use dynet_host::Private;
use secrets::{Client, Code, Phase, Report};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

pub struct Control {
    pub cache: Arc<Cache>,
    pub(super) pending: Mutex<Option<Candidate>>,
    pub(super) status: Mutex<Report>,
    pub(super) stopped: AtomicBool,
    pub(super) fatal: AtomicBool,
    pub(super) wake: (Mutex<()>, Condvar),
}

pub struct Session {
    pub control: Arc<Control>,
    pub initial: Candidate,
    pub protection: Option<Payload>,
    client: Option<Arc<Client>>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    pub fn open(
        path: &Path,
        instance: &Instance,
        state: &State,
        private: Private,
    ) -> Result<Self, Error> {
        private.folder(state.root()).map_err(custody)?;
        let legacy = state.root().join(format!("{}.ledger", instance.get()));
        if legacy.try_exists().map_err(custody)? {
            return Err(Error::new(
                "issued addresses must be explicitly migrated into the encrypted control checkpoint before startup",
            ));
        }
        let bootstrap = Bootstrap::read(path, private)?;
        let identity = bootstrap.identity(private)?;
        let trust = bootstrap.trust(&identity)?;
        let decoder = Decoder::new(bootstrap.reader()?, identity);
        let client = bootstrap.client(private, instance.mark())?.map(Arc::new);
        let cache = Arc::new(Cache::open(
            &state.root().join(format!("{}.control", instance.get())),
            trust,
            private,
        )?);
        let cached = cache.restore(&decoder)?;
        let protection = if cached
            .as_ref()
            .is_some_and(|candidate| candidate.payload.is_none())
        {
            cache
                .protection(&decoder)?
                .and_then(|candidate| candidate.payload)
        } else {
            None
        };
        let boot = format!("boot-{}-{}", std::process::id(), crate::host::clock());
        let control = Arc::new(Control {
            cache,
            pending: Mutex::new(None),
            status: Mutex::new(Report {
                device: bootstrap.target.device.clone(),
                boot,
                phase: Phase::Starting,
                received: None,
                applied: None,
                error: None,
            }),
            stopped: AtomicBool::new(false),
            fatal: AtomicBool::new(false),
            wake: (Mutex::new(()), Condvar::new()),
        });
        let worker = Worker {
            bootstrap,
            decoder,
            client: client.clone(),
            control: Arc::clone(&control),
            private,
        };
        let initial = match cached {
            Some(candidate) => candidate,
            None => match worker.control.cache.waiting(&worker.decoder)? {
                Some(candidate) => candidate,
                None => worker.prepare()?.ok_or_else(refused)?,
            },
        };
        let thread = std::thread::Builder::new()
            .name("dynet-control".to_owned())
            .spawn(move || worker.run())
            .map_err(custody)?;
        Ok(Self {
            control,
            initial,
            protection,
            client,
            thread: Some(thread),
        })
    }
}

impl Control {
    pub fn failed(&self) -> bool {
        self.fatal.load(Ordering::Acquire)
    }

    pub fn phase(&self, phase: Phase, error: Option<Code>) {
        if let Ok(mut report) = self.status.lock() {
            report.phase = phase;
            report.error = error;
        }
    }

    pub fn reconcile(
        &self,
        mut apply: impl FnMut(&Candidate) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let mut pending = self.pending.lock().map_err(|_| refused())?;
        let checkpoint = self.cache.checkpoint()?;
        if let Some(barred) = checkpoint
            .barred
            .filter(|barred| checkpoint.applied.as_ref() != Some(barred))
        {
            if let Err(error) = apply(&Candidate {
                assignment: barred,
                payload: None,
            }) {
                self.phase(Phase::Failed, Some(Code::Input));
                return Err(error);
            }
            self.phase(Phase::Revoked, None);
        }
        let Some(candidate) = pending.as_ref() else {
            return Ok(false);
        };
        if let Err(error) = apply(candidate) {
            self.phase(Phase::Failed, Some(Code::Input));
            return Err(error);
        }
        self.phase(
            if candidate.payload.is_some() {
                Phase::Applied
            } else {
                Phase::Revoked
            },
            None,
        );
        *pending = None;
        Ok(true)
    }

    pub(super) fn report(&self) -> Result<Report, Error> {
        let checkpoint = self.cache.checkpoint()?;
        let mut report = self.status.lock().map_err(|_| refused())?.clone();
        report.received = checkpoint.received.map(|assignment| assignment.receipt);
        report.applied = checkpoint.applied.map(|assignment| assignment.receipt);
        Ok(report)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.control.stopped.store(true, Ordering::Release);
        self.control.wake.1.notify_all();
        if let Some(client) = &self.client {
            client.shutdown();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
