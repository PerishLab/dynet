use super::{Bootstrap, Cache, Candidate, Control, Decoder, Source, custody, refused};
use dynet_core::Error;
use dynet_host::Private;
use secrets::{Assignment, Client, Code, Phase, Receipt};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) struct Worker {
    pub bootstrap: Bootstrap,
    pub decoder: Decoder,
    pub client: Option<Arc<Client>>,
    pub control: Arc<Control>,
    pub private: Private,
}

struct Fetched {
    bytes: Vec<u8>,
    assignment: Option<Assignment>,
}

struct Fetch<'a> {
    bootstrap: &'a Bootstrap,
    decoder: &'a Decoder,
    cache: &'a Cache,
    client: Option<&'a Client>,
    private: Private,
    fatal: &'a std::sync::atomic::AtomicBool,
    control: Option<&'a Control>,
}

impl Worker {
    pub(super) fn prepare(&self) -> Result<Option<Candidate>, Error> {
        Fetch {
            bootstrap: &self.bootstrap,
            decoder: &self.decoder,
            cache: &self.control.cache,
            client: self.client.as_deref(),
            private: self.private,
            fatal: &self.control.fatal,
            control: None,
        }
        .fetch(false)
    }

    pub fn run(self) {
        let waiting = self.control.cache.waiting(&self.decoder);
        match waiting {
            Ok(Some(candidate)) => {
                if let Ok(mut slot) = self.control.pending.lock() {
                    *slot = Some(candidate);
                }
            }
            Err(_) => {
                self.control.fatal.store(true, Ordering::Release);
                return;
            }
            _ => (),
        }
        while !self.control.stopped.load(Ordering::Acquire) {
            let ready = self
                .control
                .status
                .lock()
                .is_ok_and(|report| report.phase != Phase::Starting);
            if !ready {
                self.wait(Duration::from_millis(200));
                continue;
            }
            if let Some(client) = &self.client
                && let Ok(report) = self.control.report()
            {
                let _ = client.report(&report);
            }
            let fetch = Fetch {
                bootstrap: &self.bootstrap,
                decoder: &self.decoder,
                cache: &self.control.cache,
                client: self.client.as_deref(),
                private: self.private,
                fatal: &self.control.fatal,
                control: Some(&self.control),
            };
            match fetch.fetch(true) {
                Ok(Some(candidate)) => self.propose(candidate),
                Ok(None) => (),
                Err(_) => {
                    self.control.phase(Phase::Failed, Some(Code::Input));
                }
            }
            self.wait(Duration::from_secs(1));
        }
    }

    fn propose(&self, candidate: Candidate) {
        if let Ok(mut slot) = self.control.pending.lock() {
            *slot = Some(candidate);
            self.control.phase(Phase::Validated, None);
        }
    }

    fn wait(&self, span: Duration) {
        let Ok(guard) = self.control.wake.0.lock() else {
            return;
        };
        if !self.control.stopped.load(Ordering::Acquire) {
            let _ = self.control.wake.1.wait_timeout(guard, span);
        }
    }
}

impl Fetch<'_> {
    fn fetch(&self, watch: bool) -> Result<Option<Candidate>, Error> {
        let checkpoint = self.cache.checkpoint()?;
        let earlier = checkpoint
            .received
            .as_ref()
            .map(|assignment| &assignment.receipt);
        let found = match (&self.bootstrap.source, self.client) {
            (Source::File { path }, None) => {
                let bytes = self.private.read(path, secrets::LIMIT).map_err(custody)?;
                Some(Fetched {
                    bytes,
                    assignment: None,
                })
            }
            (Source::Url { .. }, Some(client)) => self.remote(client, earlier, watch)?,
            _ => return Err(refused()),
        };
        let Some(Fetched { bytes, assignment }) = found else {
            return Ok(None);
        };
        let incoming = self.decoder.receive(bytes, earlier)?;
        if assignment
            .as_ref()
            .is_some_and(|assignment| assignment != &incoming.assignment)
        {
            return Err(refused());
        }
        if earlier == Some(&incoming.assignment.receipt) {
            return Ok(None);
        }
        let mut slot = self
            .control
            .map(|control| control.pending.lock().map_err(|_| refused()))
            .transpose()?;
        if let Some(slot) = &mut slot {
            **slot = None;
        }
        self.cache.receive(&incoming).inspect_err(|_| {
            self.fatal.store(true, Ordering::Release);
        })?;
        let candidate = self.decoder.open(incoming)?;
        self.cache.validate(&candidate).inspect_err(|_| {
            self.fatal.store(true, Ordering::Release);
        })?;
        if let Some(slot) = &mut slot {
            **slot = Some(candidate);
            if let Some(control) = self.control {
                control.phase(Phase::Validated, None);
            }
            return Ok(None);
        }
        Ok(Some(candidate))
    }

    fn remote(
        &self,
        client: &Client,
        earlier: Option<&Receipt>,
        watch: bool,
    ) -> Result<Option<Fetched>, Error> {
        let Some(assignment) = self.assignment(client, earlier, watch)? else {
            return Ok(None);
        };
        let bytes = client.fetch(&assignment).map_err(|_| refused())?;
        Ok(Some(Fetched {
            bytes,
            assignment: Some(assignment),
        }))
    }

    fn assignment(
        &self,
        client: &Client,
        earlier: Option<&Receipt>,
        watch: bool,
    ) -> Result<Option<Assignment>, Error> {
        let device = &self.bootstrap.target.device;
        let result = if let Some(earlier) = earlier.filter(|_| watch) {
            client.watch(device, earlier)
        } else {
            client.assignment(device, earlier)
        };
        result.map_err(|_| refused())
    }
}
