use super::Fixture;
use crate::control::{Seed, Session};
use crate::host::State;
use dynet_core::{Domain, Instance, Ledger, Name};
use dynet_host::Private;
use serde_json::{Value, json};

fn issued() -> Value {
    let mut ledger = Ledger::new(86400);
    let cluster = Name::new("private").unwrap();
    for ordinal in 0..33 {
        let name = Domain::new(format!("host{ordinal}.example.com")).unwrap();
        ledger.record("8.8.8.8".parse().unwrap(), (&name, &cluster), (60, 100));
    }
    ledger.record(
        "9.9.9.9".parse().unwrap(),
        (&Domain::new("retained.example.com").unwrap(), &cluster),
        (600, 400),
    );
    let mut value: Value = serde_json::from_str(&ledger.write()).unwrap();
    value[1]["released"] = json!(true);
    value
}

#[test]
fn preserves() {
    let fixture = Fixture::new();
    let private = Private::current();
    let bootstrap = fixture.bootstrap(&fixture.content());
    let source = fixture.root.join("issued");
    let records = issued();
    private
        .create(&source, &serde_json::to_vec(&records).unwrap())
        .unwrap();
    let state = State::new(Some(fixture.root.join("output"))).unwrap();
    let instance = Instance::new("dynet0").unwrap();
    let seed = Seed {
        bootstrap: &bootstrap,
        issued: &source,
        instance: &instance,
        state: &state,
    };
    let receipt = seed.run(private).unwrap();
    let path = state.root().join("dynet0.control/checkpoint");
    let before = private.read(&path, 16384).unwrap();
    let checkpoint: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(checkpoint["ledger"], records);
    assert_eq!(
        checkpoint["received"]["receipt"],
        serde_json::to_value(&receipt).unwrap()
    );
    assert_eq!(checkpoint["validated"], checkpoint["received"]["receipt"]);
    assert!(checkpoint["applied"].is_null());
    assert!(checkpoint["retained"].is_null());
    assert!(checkpoint["barred"].is_null());
    assert_eq!(
        checkpoint["ledger"][0]["names"].as_array().unwrap().len(),
        32
    );
    assert_eq!(checkpoint["ledger"][0]["crowded"], true);
    assert_eq!(checkpoint["ledger"][1]["released"], true);
    assert_eq!(checkpoint["ledger"][0]["retained"], 86500);
    private
        .replace(
            &fixture.root.join("envelope"),
            &fixture.seal(2, &fixture.content()),
        )
        .unwrap();
    assert!(seed.run(private).is_err());
    assert_eq!(private.read(&path, 16384).unwrap(), before);
    let session = Session::open(&bootstrap, &instance, &state, private).unwrap();
    assert_eq!(session.initial.assignment.receipt, receipt);
    let ledger = session
        .control
        .cache
        .checkpoint()
        .unwrap()
        .ledger(86400)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&ledger.write()).unwrap(),
        records
    );
    assert_eq!(
        private.read(&source, 16384).unwrap(),
        serde_json::to_vec(&records).unwrap()
    );
}

#[test]
fn normalization() {
    for mode in ["reserved", "duplicate", "unknown", "shape"] {
        let fixture = Fixture::new();
        let private = Private::current();
        let bootstrap = fixture.bootstrap(&fixture.content());
        let source = fixture.root.join("issued");
        let mut records = issued();
        match mode {
            "reserved" => records[0]["address"] = json!("10.0.0.1"),
            "duplicate" => {
                let extra = records[0].clone();
                records.as_array_mut().unwrap().push(extra);
            }
            "unknown" => records[0]["ignored"] = json!(true),
            _ => records = json!({"unexpected": []}),
        }
        private
            .create(&source, &serde_json::to_vec(&records).unwrap())
            .unwrap();
        let state = State::new(Some(fixture.root.join("output"))).unwrap();
        let instance = Instance::new("dynet0").unwrap();
        assert!(
            Seed {
                bootstrap: &bootstrap,
                issued: &source,
                instance: &instance,
                state: &state
            }
            .run(private)
            .is_err()
        );
        assert!(!state.root().exists());
    }
}

#[test]
fn input() {
    for mode in ["malformed", "large", "signature", "payload", "link"] {
        let fixture = Fixture::new();
        let private = Private::current();
        let bootstrap = fixture.bootstrap(&fixture.content());
        let source = fixture.root.join("issued");
        let data = match mode {
            "large" => vec![b' '; 16 * 1024 * 1024 + 1],
            "malformed" => b"not a ledger".to_vec(),
            _ => serde_json::to_vec(&issued()).unwrap(),
        };
        private.create(&source, &data).unwrap();
        match mode {
            "signature" => {
                let mut bytes = fixture.seal(1, &fixture.content());
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                private
                    .replace(&fixture.root.join("envelope"), &bytes)
                    .unwrap();
            }
            "payload" => private
                .replace(
                    &fixture.root.join("envelope"),
                    &fixture.seal(1, b"not a payload"),
                )
                .unwrap(),
            "link" => std::fs::hard_link(&source, fixture.root.join("alias")).unwrap(),
            _ => (),
        }
        let state = State::new(Some(fixture.root.join("output"))).unwrap();
        let instance = Instance::new("dynet0").unwrap();
        assert!(
            Seed {
                bootstrap: &bootstrap,
                issued: &source,
                instance: &instance,
                state: &state
            }
            .run(private)
            .is_err()
        );
        assert!(!state.root().exists());
    }
}

#[test]
fn interrupted() {
    for blob in [false, true] {
        let fixture = Fixture::new();
        let private = Private::current();
        let bootstrap = fixture.bootstrap(&fixture.content());
        let source = fixture.root.join("issued");
        private
            .create(&source, &serde_json::to_vec(&issued()).unwrap())
            .unwrap();
        let state = State::new(Some(fixture.root.join("output"))).unwrap();
        private.folder(state.root()).unwrap();
        let root = state.root().join("dynet0.control");
        private.fresh(&root).unwrap();
        private
            .create(&root.join("initialized"), b"dynet-control-v1")
            .unwrap();
        if blob {
            private
                .create(
                    &root.join("retained.envelope"),
                    b"retained partial evidence",
                )
                .unwrap();
        }
        let instance = Instance::new("dynet0").unwrap();
        assert!(
            Seed {
                bootstrap: &bootstrap,
                issued: &source,
                instance: &instance,
                state: &state
            }
            .run(private)
            .is_err()
        );
        assert!(Session::open(&bootstrap, &instance, &state, private).is_err());
        assert!(!root.join("checkpoint").exists());
        assert_eq!(
            private.read(&root.join("initialized"), 64).unwrap(),
            b"dynet-control-v1"
        );
        if blob {
            assert_eq!(
                private.read(&root.join("retained.envelope"), 64).unwrap(),
                b"retained partial evidence"
            );
        }
    }
}
