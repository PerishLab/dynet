use dynet_api::host::Journal;
use dynet_core::{Domain, Instance, Ledger, Name};

fn journal(seat: &tempfile::TempDir) -> Journal {
    let instance = Instance::new("dynet9").expect("instance");
    let private = dynet_host::Private::current();
    let root = seat.path().canonicalize().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let cache = dynet_api::control::Cache::open(
        &root.join("dynet9.control"),
        "fixture".to_owned(),
        private,
    )
    .unwrap();
    Journal::controlled(
        &instance,
        &dynet_api::host::State::new(Some(root)).unwrap(),
        std::sync::Arc::new(cache),
    )
}

#[test]
fn keeps() {
    let seat = temporary();
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
    let seat = temporary();
    let journal = journal(&seat);
    journal.save(&Ledger::new(60)).expect("initialized");
    std::fs::write(journal.path(), "[{").expect("write");
    drop(journal);
    let refused = dynet_api::control::Cache::open(
        &seat.path().canonicalize().unwrap().join("dynet9.control"),
        "fixture".to_owned(),
        dynet_host::Private::current(),
    )
    .err()
    .expect("an unreadable checkpoint refuses");
    assert!(
        refused.to_string().contains("configuration refused"),
        "{refused}"
    );
}

#[test]
fn stains() {
    let seat = temporary();
    let journal = journal(&seat);
    assert!(!journal.stained());
    journal.stain();
    assert!(journal.stained());
    assert!(!journal.stained(), "a stain is taken once");
}

#[test]
fn isolated() {
    let seat = temporary();
    let root = seat.path().join("explicit root");
    let state = dynet_api::host::State::new(Some(root.clone())).expect("state");
    let instance = Instance::new("dynet9").expect("instance");
    let journal = Journal::new(&instance, &state);
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
    assert_eq!(journal.path(), root.join("dynet9.control/checkpoint"));
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

fn temporary() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("dynet-journal-");
    #[cfg(windows)]
    {
        builder
            .tempdir_in(dynet_host::windows::data().unwrap())
            .unwrap()
    }
    #[cfg(unix)]
    builder.tempdir().unwrap()
}
