use dynet_api::host::Journal;
use dynet_api::inbound::{Held, Store, Warren};
use dynet_api::manifest;
use dynet_api::outbound::Book;
use dynet_core::{Domain, Instance, Ledger, Name};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

const ADDRESS: &str = "203.0.113.7";

fn manifest(rules: Value) -> String {
    json!({
        "version": 1,
        "nodes": [{"label": "jm/one", "protocol": "vmess", "server": "192.0.2.1", "port": 1,
                   "uuid": "6f1c2a3b-0000-4000-8000-000000000001", "udp": true}],
        "clusters": [{"name": "proxy", "nodes": ["jm/one"]}],
        "rules": rules,
        "fallback": "direct"
    })
    .to_string()
}

#[test]
fn renews() {
    let seat = tempfile::tempdir().expect("seat");
    let source = seat.path().join("manifest.json");
    std::fs::write(
        &source,
        manifest(json!([{"suffix": "a.com", "cluster": "proxy"}])),
    )
    .expect("write");
    let loaded = manifest::read(&source).expect("manifest");
    let held = RwLock::new(Arc::new(Held::new(loaded, None).expect("held")));
    let mut issued = Ledger::default();
    let domain = Domain::new("www.a.com").expect("domain");
    let former = Name::new("former").expect("name");
    issued.record(
        ADDRESS.parse().expect("address"),
        (&domain, &former),
        (60, 0),
    );
    let ledger = Mutex::new(issued);
    let instance = Instance::new("dynet9").expect("instance");
    let journal = Journal::new(&instance, Some(seat.path().join("ledger")));
    let spoken = Mutex::new(Vec::new());
    let told = |line: &str| spoken.lock().expect("lines").push(line.to_string());
    let warren = Warren {
        instance: &instance,
        held: &held,
        ledger: &ledger,
        journal: &journal,
        source: &source,
        reload: &AtomicBool::new(false),
        port: 0,
        upstream: "192.0.2.53",
        book: &Book::new(0),
        store: &Store::new(),
        told: &told,
    };
    let rules = |warren: &Warren| warren.current().loaded.manifest().table().rules().len();
    std::fs::write(
        &source,
        manifest(json!([{"suffix": "a.com", "cluster": "nowhere"}])),
    )
    .expect("write");
    warren.renew();
    assert_eq!(rules(&warren), 1, "a bad manifest leaves the standing one");
    let two =
        json!([{"suffix": "a.com", "cluster": "proxy"}, {"exact": "b.com", "cluster": "proxy"}]);
    std::fs::write(&source, manifest(two)).expect("write");
    warren.renew();
    assert_eq!(
        rules(&warren),
        2,
        "a good manifest replaces the standing one"
    );
    let proxy = Name::new("proxy").expect("name");
    assert!(warren.current().pools.contains_key(&proxy));
    let lines = spoken.lock().expect("lines").clone();
    assert!(
        lines[0].starts_with("manifest refused, the standing one is kept"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("manifest replaced"), "{lines:?}");
    assert!(
        lines[2].starts_with("ledger re-judged moved=1 released=0"),
        "{lines:?}"
    );
    let ledger = ledger.lock().expect("ledger");
    let moved = ledger.lookup(ADDRESS.parse().expect("address"));
    assert_eq!(moved.expect("issued").cluster(), &proxy);
    assert!(journal.stained(), "a re-judged ledger is saved");
}
