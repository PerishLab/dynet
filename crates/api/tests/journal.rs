use dynet_api::host::Journal;
use dynet_core::{Domain, Instance, Ledger, Name};

fn journal(seat: &tempfile::TempDir) -> Journal {
    let instance = Instance::new("dynet9").expect("instance");
    Journal::new(
        &instance,
        &dynet_api::host::State::new(Some(seat.path().to_path_buf())).expect("state"),
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

#[test]
fn isolated() {
    let seat = tempfile::tempdir().expect("seat");
    let root = seat.path().join("explicit root");
    let state = dynet_api::host::State::new(Some(root.clone())).expect("state");
    let instance = Instance::new("dynet9").expect("instance");
    let journal = Journal::new(&instance, &state, None);
    journal.save(&Ledger::new(60)).expect("save");
    journal.ask("flush").expect("ask");
    assert_eq!(journal.taken().as_deref(), Some("flush"));
    assert!(!journal.order().exists());
    journal.reply("done").expect("reply");
    let standing = dynet_api::inbound::standing::path(&instance, &state);
    dynet_api::host::Fragment::new(standing.clone(), "snapshot")
        .write()
        .expect("standing");
    assert_eq!(journal.standing(), standing);
    assert_eq!(
        std::fs::read_to_string(journal.answer()).expect("answer"),
        "done"
    );
    assert_eq!(journal.path(), root.join("dynet9.ledger"));
    assert_eq!(standing, root.join("run/dynet9.standing"));
    assert!(root.join("run/dynet9.flushed").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn defaults() {
    let state = dynet_api::host::State::new(None).expect("state");
    assert_eq!(state.root(), std::path::Path::new("/var/lib/dynet"));
    assert_eq!(state.runtime(), std::path::Path::new("/run/dynet"));
    assert!(state.configured().is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn defaults() {
    let state = dynet_api::host::State::new(None).expect("native state");
    assert_eq!(
        state.root(),
        std::path::Path::new("/Library/Application Support/Dynet")
    );
    assert_eq!(state.runtime(), state.root().join("run"));
    assert!(state.configured().is_none());
}
