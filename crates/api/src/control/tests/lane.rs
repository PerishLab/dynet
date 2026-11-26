use super::super::cutover::Guard;
use super::super::{Private, Session};
use crate::host::State;
use dynet_core::{Instance, Ledger};
use secrets::{Envelope, ExposeSecret, Identity, Phase as Report, Signer, Target};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(super) struct Lane {
    _home: tempfile::TempDir,
    root: PathBuf,
    pub state: State,
    pub instance: Instance,
    pub bootstrap: PathBuf,
    pub target: PathBuf,
    identity: Identity,
    signer: Signer,
    targetid: Target,
}

pub(super) struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    session: Option<Session>,
}

impl Worker {
    pub(super) fn start(lane: &Lane) -> Self {
        let session = Session::open(
            &lane.bootstrap,
            &lane.instance,
            &lane.state,
            Private::current(),
        )
        .expect("session");
        let ledger = Ledger::new(86400);
        session
            .control
            .cache
            .apply(&session.initial.assignment, &ledger)
            .expect("initial apply");
        session.control.phase(Report::Applied, None);
        let control = Arc::clone(&session.control);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let ledger = Ledger::new(86400);
            while !flag.load(Ordering::Acquire) {
                let _ = control
                    .reconcile(|candidate| control.cache.apply(&candidate.assignment, &ledger));
                thread::sleep(Duration::from_millis(50));
            }
        });
        Self {
            stop,
            handle: Some(handle),
            session: Some(session),
        }
    }

    pub(super) fn applied(&self) -> u64 {
        self.session
            .as_ref()
            .unwrap()
            .control
            .cache
            .checkpoint()
            .unwrap()
            .applied
            .unwrap()
            .receipt
            .generation
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        drop(self.session.take());
    }
}

impl Lane {
    pub(super) fn new(name: &str) -> Self {
        let mut builder = tempfile::Builder::new();
        builder.prefix("dynet-cutover-");
        builder.permissions(std::fs::Permissions::from_mode(0o700));
        let home = builder.tempdir().unwrap();
        let root = home.path().canonicalize().unwrap().join("private");
        let private = Private::current();
        private.folder(&root).unwrap();
        let statedir = root.join("state");
        private.folder(&statedir).unwrap();
        let identity = Identity::generate();
        let signer = Signer::generate().unwrap();
        let targetid = Target::new(name, "routing", "epoch1").unwrap();
        let target = root.join("live.envelope");
        let bootstrap = root.join("bootstrap.json");
        let key = root.join("identity.age");
        private
            .create(&key, identity.export().expose_secret().as_bytes())
            .unwrap();
        let doc = json!({
            "version": 1,
            "target": targetid,
            "identityRef": key,
            "publisher": signer.publisher().unwrap().to_string(),
            "minimum": 1,
            "source": {"kind": "file", "path": target}
        });
        private
            .create(&bootstrap, &serde_json::to_vec(&doc).unwrap())
            .unwrap();
        let lane = Self {
            _home: home,
            root,
            state: State::new(Some(statedir)).unwrap(),
            instance: Instance::new(name).unwrap(),
            bootstrap,
            target,
            identity,
            signer,
            targetid,
        };
        let base = lane.seal(1, &payload("base", "1.1.1.1"));
        private.create(&lane.target, &base).unwrap();
        lane
    }

    pub(super) fn seal(&self, generation: u64, body: &Value) -> Vec<u8> {
        Envelope::seal(
            self.targetid.clone(),
            generation,
            &serde_json::to_vec(body).unwrap(),
            (&self.identity.recipient(), &self.signer),
        )
        .unwrap()
    }

    pub(super) fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        Private::current().replace(&path, bytes).unwrap();
        path
    }

    pub(super) fn guard(&self) -> Guard {
        Guard::new(&self.instance, &self.state, Private::current())
    }

    pub(super) fn cutover(&self) -> PathBuf {
        self.state
            .root()
            .join(format!("{}.cutover", self.instance.get()))
    }
}

pub(super) fn payload(label: &str, upstream: &str) -> Value {
    json!({
        "version": 1,
        "upstream": upstream,
        "retain": 86400,
        "manifest": {
            "version": 1,
            "generation": label,
            "nodes": [],
            "clusters": [],
            "rules": [],
            "fallback": "direct"
        }
    })
}

pub(super) fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}
