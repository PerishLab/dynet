use super::Fixture;
use crate::control::{Candidate, Private, Projection, Session};
use crate::host::{Journal, State};
use crate::inbound::{Held, Store, Warren};
use crate::outbound::Book;
use dynet_core::{Bearing, Domain, Error, Instance, Ledger, Name, Range, Verdict};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

#[derive(Default)]
struct Host {
    failed: AtomicBool,
    calls: Mutex<Vec<BTreeSet<Range>>>,
}

impl Projection for Host {
    fn settle(&self, wanted: &BTreeSet<Range>) -> Result<(), Error> {
        self.calls.lock().unwrap().push(wanted.clone());
        if self.failed.load(Ordering::Relaxed) {
            return Err(Error::new("injected projection failure"));
        }
        Ok(())
    }
}

struct Rig {
    session: Session,
    instance: Instance,
    held: RwLock<Arc<Held>>,
    ledger: Mutex<Ledger>,
    journal: Journal,
    host: Host,
    halt: AtomicBool,
    reload: AtomicBool,
    book: Book,
    store: Store,
}

impl Rig {
    fn new(fixture: &Fixture) -> Self {
        let state = State::new(Some(fixture.root.clone())).unwrap();
        let instance = Instance::new("dynet9").unwrap();
        let bootstrap = fixture.bootstrap(&policy(false));
        let session = Session::open(&bootstrap, &instance, &state, Private::current()).unwrap();
        let mut held = Held::new(session.initial.payload.clone().unwrap(), None).unwrap();
        held.stamp = session.initial.assignment.receipt.digest.clone();
        let mut ledger = Ledger::new(86400);
        ledger.record(
            "203.0.113.7".parse().unwrap(),
            (
                &Domain::new("www.a.com").unwrap(),
                &Name::new("former").unwrap(),
            ),
            (60, 0),
        );
        let journal = Journal::controlled(&instance, &state, Arc::clone(&session.control.cache));
        Self {
            session,
            instance,
            held: RwLock::new(Arc::new(held)),
            ledger: Mutex::new(ledger),
            journal,
            host: Host::default(),
            halt: AtomicBool::new(false),
            reload: AtomicBool::new(false),
            book: Book::new(0),
            store: Store::new(),
        }
    }

    fn warren(&self) -> Warren<'_> {
        Warren {
            instance: &self.instance,
            initial: &self.session.initial.assignment,
            held: &self.held,
            ledger: &self.ledger,
            journal: &self.journal,
            control: &self.session.control,
            projection: &self.host,
            reload: &self.reload,
            halt: &self.halt,
            port: 0,
            book: &self.book,
            store: &self.store,
            told: &quiet,
        }
    }

    fn candidate(&self, fixture: &Fixture, generation: u64, content: &[u8]) -> Candidate {
        let decoder = fixture.decoder();
        let checkpoint = self.session.control.cache.checkpoint().unwrap();
        let incoming = decoder
            .receive(
                fixture.seal(generation, content),
                checkpoint
                    .received
                    .as_ref()
                    .map(|assignment| &assignment.receipt),
            )
            .unwrap();
        self.session.control.cache.receive(&incoming).unwrap();
        let candidate = decoder.open(incoming).unwrap();
        self.session.control.cache.validate(&candidate).unwrap();
        candidate
    }
}

fn quiet(_: &str) {}

fn policy(extra: bool) -> Vec<u8> {
    let mut rules = vec![serde_json::json!({"suffix":"a.com","cluster":"proxy"})];
    if extra {
        rules.push(serde_json::json!({"suffix":"b.com","cluster":"proxy"}));
    }
    serde_json::to_vec(&serde_json::json!({"version":1,"upstream":"1.1.1.1","retain":86400,"manifest":{
        "version":1,"nodes":[{"label":"jm/one","protocol":"vmess","server":"192.0.2.1","port":1,"uuid":"6f1c2a3b-0000-4000-8000-000000000001","udp":true}],
        "clusters":[{"name":"proxy","nodes":["jm/one"]}],"rules":rules,"fallback":"direct"
    }})).unwrap()
}

#[test]
fn confirmation() {
    let fixture = Fixture::new();
    let rig = Rig::new(&fixture);
    let warren = rig.warren();
    warren.apply(&rig.session.initial).unwrap();
    let held = warren.current();
    let proxy = Name::new("proxy").unwrap();
    let label = dynet_core::Label::new("jm/one").unwrap();
    held.pools[&proxy].lock().unwrap().observed(
        &label,
        Bearing::Stream,
        Verdict::Answered,
        Instant::now(),
    );
    let candidate = rig.candidate(&fixture, 2, &policy(true));
    rig.host.failed.store(true, Ordering::Relaxed);
    assert!(warren.apply(&candidate).is_err());
    assert_eq!(warren.current().loaded.manifest().table().rules().len(), 1);
    assert_eq!(
        rig.session
            .control
            .cache
            .checkpoint()
            .unwrap()
            .applied
            .unwrap()
            .receipt
            .generation,
        1
    );
    assert!(!rig.halt.load(Ordering::Relaxed));
    rig.host.failed.store(false, Ordering::Relaxed);
    warren.apply(&candidate).unwrap();
    assert_eq!(warren.current().loaded.manifest().table().rules().len(), 2);
    assert_eq!(
        rig.session
            .control
            .cache
            .checkpoint()
            .unwrap()
            .applied
            .unwrap()
            .receipt
            .generation,
        2
    );
    assert_eq!(
        warren.current().pools[&proxy]
            .lock()
            .unwrap()
            .survey(Bearing::Stream)[0]
            .1
            .answered(),
        1
    );
    assert_eq!(
        rig.ledger
            .lock()
            .unwrap()
            .lookup("203.0.113.7".parse().unwrap())
            .unwrap()
            .cluster(),
        &proxy
    );
}

#[test]
fn revocation() {
    let fixture = Fixture::new();
    let rig = Rig::new(&fixture);
    let warren = rig.warren();
    warren.apply(&rig.session.initial).unwrap();
    let previous = warren.current();
    let circuit = previous.fence.ticket().unwrap();
    let decoder = fixture.decoder();
    let bytes = secrets::Envelope::revoke(fixture.target.clone(), 9, &fixture.signer).unwrap();
    let incoming = decoder
        .receive(bytes, Some(&rig.session.initial.assignment.receipt))
        .unwrap();
    rig.session.control.cache.receive(&incoming).unwrap();
    let incoming = decoder
        .receive(fixture.seal(10, b"invalid configuration"), None)
        .unwrap();
    rig.session.control.cache.receive(&incoming).unwrap();
    assert!(decoder.open(incoming).is_err());
    warren.renew();
    assert!(warren.current().paused);
    assert!(circuit.closed());
    assert!(previous.fence.ticket().is_err());
    let checkpoint = rig.session.control.cache.checkpoint().unwrap();
    assert_eq!(checkpoint.applied.unwrap().receipt.generation, 9);
    assert_eq!(checkpoint.received.unwrap().receipt.generation, 10);
    warren.forget(&BTreeMap::new());
    assert_eq!(rig.ledger.lock().unwrap().addresses().count(), 1);
    let recovered = rig.candidate(&fixture, 11, &policy(true));
    warren.apply(&recovered).unwrap();
    assert!(!warren.current().paused);
    assert!(warren.current().fence.ticket().is_ok());
    assert!(circuit.closed());
    assert!(
        rig.session
            .control
            .cache
            .checkpoint()
            .unwrap()
            .barred
            .is_none()
    );
}

#[test]
fn durability() {
    let fixture = Fixture::new();
    let rig = Rig::new(&fixture);
    let warren = rig.warren();
    warren.apply(&rig.session.initial).unwrap();
    let previous = warren.current();
    let circuit = previous.fence.ticket().unwrap();
    let candidate = rig.candidate(&fixture, 2, &policy(true));
    let path = fixture.root.join("dynet9.control/checkpoint");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(warren.apply(&candidate).is_err());
    assert!(rig.halt.load(Ordering::Relaxed));
    assert!(circuit.closed());
    assert_eq!(warren.current().loaded.manifest().table().rules().len(), 1);
    assert_eq!(
        rig.session
            .control
            .cache
            .checkpoint()
            .unwrap()
            .applied
            .unwrap()
            .receipt
            .generation,
        1
    );
}
