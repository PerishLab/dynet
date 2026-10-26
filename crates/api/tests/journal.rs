use dynet_api::host::Journal;
use dynet_core::{Domain, Instance, Ledger, Name};

fn journal(seat: &tempfile::TempDir) -> Journal {
    let instance = Instance::new("dynet9").expect("instance");
    Journal::new(
        &instance,
        Some(seat.path().join("state").join("dynet9.ledger")),
    )
}

#[test]
fn keeps() {
    let seat = tempfile::tempdir().expect("seat");
    let journal = journal(&seat);
    assert!(
        journal
            .load(60)
            .expect("absent")
            .addresses()
            .next()
            .is_none(),
        "no ledger starts empty"
    );
    let mut ledger = Ledger::new(60);
    let domain = Domain::new("a.example.com").expect("domain");
    let cluster = Name::new("tunnel").expect("name");
    ledger.record(
        "203.0.113.1".parse().expect("address"),
        (&domain, &cluster),
        (30, 1000),
    );
    journal.save(&ledger).expect("save");
    assert!(
        !journal.path().with_extension("part").exists(),
        "the write is renamed into place"
    );
    assert_eq!(journal.load(60).expect("load").write(), ledger.write());
}

#[test]
fn refuses() {
    let seat = tempfile::tempdir().expect("seat");
    let journal = journal(&seat);
    std::fs::create_dir_all(journal.path().parent().expect("parent")).expect("dir");
    std::fs::write(journal.path(), "[{").expect("write");
    let refused = journal.load(60).expect_err("an unreadable ledger refuses");
    assert!(
        refused.to_string().contains("refuses to start"),
        "{refused}"
    );
}

#[test]
fn stains() {
    let seat = tempfile::tempdir().expect("seat");
    let journal = journal(&seat);
    assert!(!journal.stained());
    journal.stain();
    assert!(journal.stained());
    assert!(!journal.stained(), "a stain is taken once");
}
