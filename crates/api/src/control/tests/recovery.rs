use super::Fixture;
use crate::control::{Cache, Private};
use dynet_core::Ledger;
use secrets::{Envelope, Kind};

#[test]
fn barred() {
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
    let bytes = Envelope::revoke(fixture.target.clone(), 9, &fixture.signer).unwrap();
    let incoming = decoder.receive(bytes, None).unwrap();
    cache.receive(&incoming).unwrap();
    let incoming = decoder.receive(fixture.seal(10, b"invalid"), None).unwrap();
    cache.receive(&incoming).unwrap();
    assert!(decoder.open(incoming).is_err());
    drop(cache);
    let cache = fixture.cache();
    let barred = cache.restore(&decoder).unwrap().unwrap();
    assert_eq!(barred.assignment.kind, Kind::Revocation);
    assert_eq!(barred.assignment.receipt.generation, 9);
    assert_eq!(
        cache
            .checkpoint()
            .unwrap()
            .applied
            .unwrap()
            .receipt
            .generation,
        1
    );
    cache
        .apply(&barred.assignment, &Ledger::new(86400))
        .unwrap();
    assert_eq!(
        cache
            .checkpoint()
            .unwrap()
            .received
            .unwrap()
            .receipt
            .generation,
        10
    );
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
    let incoming = decoder
        .receive(fixture.seal(11, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    cache.validate(&candidate).unwrap();
    cache
        .apply(&candidate.assignment, &Ledger::new(86400))
        .unwrap();
    assert!(cache.checkpoint().unwrap().barred.is_none());
    assert_eq!(
        cache
            .restore(&decoder)
            .unwrap()
            .unwrap()
            .assignment
            .receipt
            .generation,
        11
    );
}

#[test]
fn missing() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let cache = fixture.cache();
    let incoming = decoder
        .receive(fixture.seal(1, &fixture.content()), None)
        .unwrap();
    cache.receive(&incoming).unwrap();
    std::fs::remove_file(fixture.root.join("cache/checkpoint")).unwrap();
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
