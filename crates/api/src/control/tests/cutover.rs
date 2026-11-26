use super::super::Private;
use super::super::cutover::{Arm, Guard, Phase};
use super::lane::{Lane, Worker, mode, payload};
use serde_json::json;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn confirm() {
    let lane = Lane::new("dynetsplit");
    let worker = Worker::start(&lane);
    assert_eq!(worker.applied(), 1);
    let candidate = lane.write("cand.env", &lane.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = lane.write("back.env", &lane.seal(3, &payload("base", "1.1.1.1")));
    let guard = lane.guard();
    let armed = guard
        .arm(&Arm {
            bootstrap: &lane.bootstrap,
            candidate: &candidate,
            rollback: &rollback,
            window: 30,
            settle: 10,
        })
        .expect("armed");
    assert_eq!(armed.phase, Phase::Armed);
    assert_eq!(mode(&lane.cutover()), 0o700);
    assert_eq!(mode(&lane.cutover().join("state.json")), 0o600);
    assert_eq!(mode(&lane.cutover().join("rollback.envelope")), 0o600);

    let applying = guard.begin(&armed.id).expect("begin");
    assert_eq!(applying.phase, Phase::Applying);
    let id = armed.id.clone();
    let watcher = Guard::new(&lane.instance, &lane.state, Private::current());
    let thread = thread::spawn(move || watcher.watch(&id, Some(10)));
    let active = guard.activate(&armed.id, Some(10)).expect("activate");
    assert_eq!(active.phase, Phase::Active);
    assert_eq!(worker.applied(), 2);

    assert!(guard.confirm(&"0".repeat(32)).is_err());
    let confirmed = guard.confirm(&armed.id).expect("confirm");
    assert_eq!(confirmed.phase, Phase::Confirmed);
    assert!(!lane.cutover().join("rollback.envelope").exists());
    assert!(!lane.cutover().join("candidate.envelope").exists());
    assert!(guard.confirm(&armed.id).is_err());
    assert_eq!(
        thread.join().unwrap().unwrap().unwrap().phase,
        Phase::Confirmed
    );
    assert_eq!(worker.applied(), 2);
}

#[test]
fn expiry() {
    let lane = Lane::new("dynetpixel");
    let worker = Worker::start(&lane);
    let candidate = lane.write("cand.env", &lane.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = lane.write("back.env", &lane.seal(3, &payload("base", "1.1.1.1")));
    let guard = lane.guard();
    let armed = guard
        .arm(&Arm {
            bootstrap: &lane.bootstrap,
            candidate: &candidate,
            rollback: &rollback,
            window: 2,
            settle: 10,
        })
        .expect("armed");
    guard.begin(&armed.id).expect("begin");
    let id = armed.id.clone();
    let watcher = Guard::new(&lane.instance, &lane.state, Private::current());
    let thread = thread::spawn(move || watcher.watch(&id, Some(10)));
    guard.activate(&armed.id, Some(10)).expect("activate");
    assert_eq!(worker.applied(), 2);

    let reverted = thread.join().unwrap().unwrap().unwrap();
    assert_eq!(reverted.phase, Phase::Reverted);
    assert_eq!(worker.applied(), 3);
    assert!(!lane.cutover().join("rollback.envelope").exists());
    assert!(guard.confirm(&armed.id).is_err());
}

#[test]
fn rollback() {
    let lane = Lane::new("dynetsplit");
    let worker = Worker::start(&lane);
    let candidate = lane.write("cand.env", &lane.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = lane.write("back.env", &lane.seal(3, &payload("base", "1.1.1.1")));
    let guard = lane.guard();
    let armed = guard
        .arm(&Arm {
            bootstrap: &lane.bootstrap,
            candidate: &candidate,
            rollback: &rollback,
            window: 30,
            settle: 10,
        })
        .expect("armed");
    guard.begin(&armed.id).expect("begin");
    let id = armed.id.clone();
    let watcher = Guard::new(&lane.instance, &lane.state, Private::current());
    let thread = thread::spawn(move || watcher.watch(&id, Some(10)));
    guard.activate(&armed.id, Some(10)).expect("activate");
    assert_eq!(worker.applied(), 2);

    let reverted = guard.rollback(&armed.id, Some(10)).expect("rollback");
    assert_eq!(reverted.phase, Phase::Reverted);
    assert_eq!(worker.applied(), 3);
    assert_eq!(
        thread.join().unwrap().unwrap().unwrap().phase,
        Phase::Reverted
    );
}

#[test]
fn refused() {
    let lane = Lane::new("dynetsplit");
    let baseline = std::fs::read(&lane.target).unwrap();
    let guard = lane.guard();

    let mut bad = payload("cand", "8.8.8.8");
    bad["manifest"]["generation"] = json!(25);
    let malformed = lane.write("bad.env", &lane.seal(2, &bad));
    let rollback = lane.write("back.env", &lane.seal(3, &payload("base", "1.1.1.1")));
    let error = guard
        .arm(&Arm {
            bootstrap: &lane.bootstrap,
            candidate: &malformed,
            rollback: &rollback,
            window: 30,
            settle: 10,
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("manifest.generation"), "{error}");
    assert!(error.contains("--generation"), "{error}");

    let good = lane.write("good.env", &lane.seal(2, &payload("cand", "8.8.8.8")));
    let wrong = lane.write("wrong.env", &lane.seal(4, &payload("base", "1.1.1.1")));
    assert!(
        guard
            .arm(&Arm {
                bootstrap: &lane.bootstrap,
                candidate: &good,
                rollback: &wrong,
                window: 30,
                settle: 10,
            })
            .unwrap_err()
            .to_string()
            .contains("candidate generation + 1")
    );

    let drifted = lane.write("drift.env", &lane.seal(3, &payload("other", "9.9.9.9")));
    assert!(
        guard
            .arm(&Arm {
                bootstrap: &lane.bootstrap,
                candidate: &good,
                rollback: &drifted,
                window: 30,
                settle: 10,
            })
            .unwrap_err()
            .to_string()
            .contains("does not match current confirmed policy")
    );

    let stale = lane.write("stale.env", &lane.seal(1, &payload("cand", "8.8.8.8")));
    let back = lane.write("staleback.env", &lane.seal(2, &payload("base", "1.1.1.1")));
    assert!(
        guard
            .arm(&Arm {
                bootstrap: &lane.bootstrap,
                candidate: &stale,
                rollback: &back,
                window: 30,
                settle: 10,
            })
            .unwrap_err()
            .to_string()
            .contains("generation floor")
    );
    assert_eq!(std::fs::read(&lane.target).unwrap(), baseline);
}

#[test]
fn retained() {
    let lane = Lane::new("dynetsplit");
    let worker = Worker::start(&lane);
    drop(worker);
    let candidate = lane.write("cand.env", &lane.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = lane.write("back.env", &lane.seal(3, &payload("base", "1.1.1.1")));
    let guard = lane.guard();
    let armed = guard
        .arm(&Arm {
            bootstrap: &lane.bootstrap,
            candidate: &candidate,
            rollback: &rollback,
            window: 1,
            settle: 1,
        })
        .expect("armed");
    guard.begin(&armed.id).expect("begin");
    guard.ready(&armed.id, std::process::id()).unwrap();
    guard.activate(&armed.id, Some(1)).expect("activate");
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if guard.reconcile(Some(1)).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let state = guard.status().unwrap().unwrap();
    assert_eq!(state.phase, Phase::Unknown);
    assert!(lane.cutover().join("rollback.envelope").exists());
    assert!(
        guard
            .arm(&Arm {
                bootstrap: &lane.bootstrap,
                candidate: &candidate,
                rollback: &rollback,
                window: 10,
                settle: 5,
            })
            .is_err()
    );
    let cleared = guard.clear(&armed.id).expect("clear");
    assert_eq!(cleared.phase, Phase::Unknown);
    assert!(guard.status().unwrap().is_none());
}
