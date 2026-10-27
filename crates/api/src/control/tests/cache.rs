use super::Fixture;
use crate::control::Cache;
use dynet_core::{Domain, Ledger, Name};
use dynet_host::Private;
use secrets::{Envelope, Kind};

#[test]
fn restart() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let cache = fixture.cache();
    let incoming = decoder
        .receive(fixture.seal(5, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    cache.validate(&candidate).unwrap();
    let mut ledger = Ledger::new(86400);
    ledger.record(
        "8.8.8.8".parse().unwrap(),
        (
            &Domain::new("example.com").unwrap(),
            &Name::new("private").unwrap(),
        ),
        (60, 100),
    );
    cache.apply(&candidate.assignment, &ledger).unwrap();
    let applied = candidate.assignment;
    drop(cache);
    let cache = fixture.cache();
    assert_eq!(
        cache.restore(&decoder).unwrap().unwrap().assignment,
        applied
    );
    assert!(
        cache
            .checkpoint()
            .unwrap()
            .ledger(86400)
            .unwrap()
            .lookup("8.8.8.8".parse().unwrap())
            .is_some()
    );
    assert!(
        Cache::open(
            &fixture.root.join("cache"),
            "trust".to_owned(),
            Private::current()
        )
        .is_err()
    );
    drop(cache);
    assert!(
        Cache::open(
            &fixture.root.join("cache"),
            "other".to_owned(),
            Private::current()
        )
        .is_err()
    );
}

#[test]
fn rejection() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let cache = fixture.cache();
    let incoming = decoder
        .receive(fixture.seal(5, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    cache.validate(&candidate).unwrap();
    cache
        .apply(&candidate.assignment, &Ledger::new(86400))
        .unwrap();
    let invalid = fixture.seal(9, b"private-credential-invalid-payload");
    let incoming = decoder
        .receive(invalid, Some(&candidate.assignment.receipt))
        .unwrap();
    cache.receive(&incoming).unwrap();
    assert!(decoder.open(incoming).is_err());
    let checkpoint = cache.checkpoint().unwrap();
    let floor = &checkpoint.received.as_ref().unwrap().receipt;
    assert_eq!(floor.generation, 9);
    assert!(
        decoder
            .receive(fixture.seal(8, &fixture.content()), Some(floor))
            .is_err()
    );
    assert!(
        decoder
            .receive(fixture.seal(9, &fixture.content()), Some(floor))
            .is_err()
    );
    drop(cache);
    let cache = fixture.cache();
    assert_eq!(
        cache
            .restore(&decoder)
            .unwrap()
            .unwrap()
            .assignment
            .receipt
            .generation,
        5
    );
    let raw = std::fs::read(fixture.root.join("cache/checkpoint")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("private-credential"));
    assert!(!String::from_utf8_lossy(&raw).contains("upstream"));
}

#[test]
fn revocation() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let cache = fixture.cache();
    let incoming = decoder
        .receive(fixture.seal(1, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    cache.validate(&candidate).unwrap();
    cache
        .apply(&candidate.assignment, &Ledger::new(86400))
        .unwrap();
    let bytes = Envelope::revoke(fixture.target.clone(), 2, &fixture.signer).unwrap();
    let incoming = decoder
        .receive(bytes, Some(&candidate.assignment.receipt))
        .unwrap();
    cache.receive(&incoming).unwrap();
    let revoked = decoder.open(incoming).unwrap();
    cache.validate(&revoked).unwrap();
    cache
        .apply(&revoked.assignment, &Ledger::new(86400))
        .unwrap();
    drop(cache);
    let cache = fixture.cache();
    let restored = cache.restore(&decoder).unwrap().unwrap();
    assert_eq!(restored.assignment.kind, Kind::Revocation);
    assert!(restored.payload.is_none());
    assert_eq!(
        cache
            .protection(&decoder)
            .unwrap()
            .unwrap()
            .assignment
            .receipt
            .generation,
        1
    );
    let count = std::fs::read_dir(fixture.root.join("cache"))
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "envelope")
        })
        .count();
    assert_eq!(count, 2);
}

#[test]
fn confirmation() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let cache = fixture.cache();
    let incoming = decoder
        .receive(fixture.seal(1, &fixture.content()), None)
        .unwrap();
    let candidate = decoder.open(incoming).unwrap();
    assert!(cache.validate(&candidate).is_err());
    assert!(
        cache
            .apply(&candidate.assignment, &Ledger::new(86400))
            .is_err()
    );
    assert!(cache.restore(&decoder).unwrap().is_none());
    let incoming = decoder
        .receive(fixture.seal(2, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    assert!(
        cache
            .apply(&candidate.assignment, &Ledger::new(86400))
            .is_err()
    );
    cache.validate(&candidate).unwrap();
    std::fs::remove_file(fixture.root.join("cache/checkpoint")).unwrap();
    std::fs::create_dir(fixture.root.join("cache/checkpoint")).unwrap();
    assert!(
        cache
            .apply(&candidate.assignment, &Ledger::new(86400))
            .is_err()
    );
    assert!(cache.checkpoint().unwrap().applied.is_none());
    drop(cache);
    assert!(
        Cache::open(
            &fixture.root.join("cache"),
            "trust".to_owned(),
            Private::current()
        )
        .is_err()
    );
}
