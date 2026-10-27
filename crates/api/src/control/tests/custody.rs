use super::Fixture;
use crate::control::Payload;
use secrets::{Envelope, Signer, Target};

#[test]
fn authentication() {
    let fixture = Fixture::new();
    let decoder = fixture.decoder();
    let bytes = fixture.seal(1, &fixture.content());
    let mut changed = bytes.clone();
    changed[20] ^= 1;
    assert!(decoder.receive(changed, None).is_err());
    let foreign = Signer::generate().unwrap();
    let bytes = Envelope::seal(
        fixture.target.clone(),
        1,
        &fixture.content(),
        (&fixture.identity.recipient(), &foreign),
    )
    .unwrap();
    assert!(decoder.receive(bytes, None).is_err());
    let bytes = Envelope::seal(
        Target::new("another", "routing", "enrollment1").unwrap(),
        1,
        &fixture.content(),
        (&fixture.identity.recipient(), &fixture.signer),
    )
    .unwrap();
    assert!(decoder.receive(bytes, None).is_err());
    let cache = fixture.cache();
    let bytes = fixture.seal(1, &fixture.content());
    let incoming = decoder.receive(bytes, None).unwrap();
    let receipt = incoming.assignment.receipt.clone();
    cache.receive(&incoming).unwrap();
    let candidate = decoder.open(incoming).unwrap();
    cache.validate(&candidate).unwrap();
    cache
        .apply(&candidate.assignment, &dynet_core::Ledger::new(86400))
        .unwrap();
    std::fs::write(
        fixture
            .root
            .join(format!("cache/{}.envelope", receipt.digest)),
        b"corrupt",
    )
    .unwrap();
    assert!(cache.restore(&decoder).is_err());
}

#[test]
fn payload() {
    let fixture = Fixture::new();
    let mut value: serde_json::Value = serde_json::from_slice(&fixture.content()).unwrap();
    value["manifest"]["nodes"] =
        serde_json::json!([{"label":"sensitive-material","protocol":"unknown"}]);
    value["manifest"]["fallback"] = serde_json::json!("sensitive-material");
    let bytes = serde_json::to_vec(&value).unwrap();
    let error = Payload::read(&bytes).err().unwrap().to_string();
    assert!(!error.contains("sensitive-material"));
    value["version"] = serde_json::json!(2);
    assert!(Payload::read(&serde_json::to_vec(&value).unwrap()).is_err());
}
