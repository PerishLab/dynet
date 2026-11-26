#![cfg(target_os = "linux")]

#[path = "cutover/fixture.rs"]
mod fixture;

use fixture::{Fixture, Live, parse, payload};
use serde_json::json;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn custody() {
    let fixture = Fixture::new("dynetsplit");
    let candidate = fixture.write("cand.env", &fixture.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = fixture.write("back.env", &fixture.seal(3, &payload("base", "1.1.1.1")));
    let output = fixture.run(&[
        "arm",
        "--bootstrap",
        fixture.bootstrap.to_str().unwrap(),
        "--candidate",
        candidate.to_str().unwrap(),
        "--rollback",
        rollback.to_str().unwrap(),
        "--window",
        "30",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("private cutover state refused"));
}

#[test]
fn confirm() {
    let fixture = Fixture::new("dynetsplit");
    let live = Live::start(&fixture);
    let candidate = fixture.write("cand.env", &fixture.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = fixture.write("back.env", &fixture.seal(3, &payload("base", "1.1.1.1")));
    let armed = parse(&fixture.run(&[
        "arm",
        "--bootstrap",
        fixture.bootstrap.to_str().unwrap(),
        "--candidate",
        candidate.to_str().unwrap(),
        "--rollback",
        rollback.to_str().unwrap(),
        "--window",
        "30",
        "--settle",
        "10",
        "--user",
    ]));
    let id = armed["id"].as_str().unwrap();
    let applied = parse(&fixture.run(&["apply", "--id", id, "--settle", "10", "--user"]));
    assert_eq!(applied["phase"], "active");
    assert_eq!(live.applied(), 2);

    let wrong = fixture.run(&["confirm", "--id", &"a".repeat(32), "--user"]);
    assert_eq!(wrong.status.code(), Some(2));

    let confirmed = parse(&fixture.run(&["confirm", "--id", id, "--user"]));
    assert_eq!(confirmed["phase"], "confirmed");
    assert_eq!(live.applied(), 2);
    assert!(
        !fixture
            .statedir
            .join("dynetsplit.cutover/rollback.envelope")
            .exists()
    );
    let repeat = fixture.run(&["confirm", "--id", id, "--user"]);
    assert_eq!(repeat.status.code(), Some(2));
}

#[test]
fn disconnect() {
    let fixture = Fixture::new("dynetpixel");
    let live = Live::start(&fixture);
    let candidate = fixture.write("cand.env", &fixture.seal(2, &payload("cand", "8.8.8.8")));
    let rollback = fixture.write("back.env", &fixture.seal(3, &payload("base", "1.1.1.1")));
    let armed = parse(&fixture.run(&[
        "arm",
        "--bootstrap",
        fixture.bootstrap.to_str().unwrap(),
        "--candidate",
        candidate.to_str().unwrap(),
        "--rollback",
        rollback.to_str().unwrap(),
        "--window",
        "2",
        "--settle",
        "10",
        "--user",
    ]));
    let id = armed["id"].as_str().unwrap();

    let script = format!(
        "'{}' --instance dynetpixel --state '{}' cutover apply --id '{}' --settle 10 --user && kill -HUP $$",
        env!("CARGO_BIN_EXE_dynet"),
        fixture.statedir.to_str().unwrap(),
        id
    );
    let ran = Command::new("/bin/sh")
        .env("HOME", &fixture.root)
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(!ran.status.success());
    assert_eq!(live.applied(), 2);

    let until = Instant::now() + Duration::from_secs(12);
    let mut phase = String::new();
    while Instant::now() < until {
        let status = parse(&fixture.run(&["status", "--user"]));
        phase = status["phase"].as_str().unwrap().to_owned();
        if phase == "reverted" {
            break;
        }
        thread::sleep(Duration::from_millis(150));
    }
    assert_eq!(phase, "reverted");
    assert_eq!(live.applied(), 3);
    assert!(
        !fixture
            .statedir
            .join("dynetpixel.cutover/rollback.envelope")
            .exists()
    );
    let late = fixture.run(&["confirm", "--id", id, "--user"]);
    assert_eq!(late.status.code(), Some(2));
}

#[test]
fn malformed() {
    let fixture = Fixture::new("dynetsplit");
    let baseline = std::fs::read(&fixture.target).unwrap();
    let mut bad = payload("cand", "8.8.8.8");
    bad["manifest"]["generation"] = json!(25);
    let candidate = fixture.write("bad.env", &fixture.seal(2, &bad));
    let rollback = fixture.write("back.env", &fixture.seal(3, &payload("base", "1.1.1.1")));
    let output = fixture.run(&[
        "arm",
        "--bootstrap",
        fixture.bootstrap.to_str().unwrap(),
        "--candidate",
        candidate.to_str().unwrap(),
        "--rollback",
        rollback.to_str().unwrap(),
        "--window",
        "30",
        "--user",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("manifest.generation"), "{stderr}");
    assert!(stderr.contains("--generation"), "{stderr}");
    assert_eq!(std::fs::read(&fixture.target).unwrap(), baseline);
}
