#[path = "observation/fixture.rs"]
mod fixture;

use fixture::Fixture;

const ENABLED: &str = "[locus]\nenabled = true\nendpoint = 'http://127.0.0.1:0'\n";

#[test]
fn disabled() {
    let fixture = Fixture::new();
    let absent = fixture.run("reach");
    fixture.configure("[locus]\nenabled = false\n");
    let disabled = fixture.run("reach");
    assert_eq!(absent.status.code(), Some(0));
    assert_eq!(absent.status.code(), disabled.status.code());
    assert_eq!(absent.stdout, disabled.stdout);
    assert_eq!(absent.stderr, disabled.stderr);
    assert!(!fixture.seat().join("state/locus").exists());
}

#[test]
fn buffered() {
    let fixture = Fixture::new();
    fixture.configure(ENABLED);
    for (name, code) in [
        ("doctor", 2),
        ("up", 2),
        ("down", 2),
        ("standing", 2),
        ("resolve", 0),
        ("reach", 0),
        ("spread", 1),
        ("identity", 2),
        ("divert.unit", 0),
    ] {
        let result = fixture.run(name);
        assert_eq!(
            result.status.code(),
            Some(code),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let atoms = fixture.records();
    assert!(atoms.iter().all(|atom| atom["producer"] == "dynet"));
    for name in [
        "doctor",
        "up",
        "down",
        "standing",
        "resolve",
        "reach",
        "spread",
        "identity",
        "divert.unit",
    ] {
        assert!(
            atoms
                .iter()
                .any(|atom| atom["context"]["dynet.command"] == name)
        );
    }
    for code in [0, 1, 2] {
        assert!(atoms.iter().any(
            |atom| atom["payload"]["event"] == "cli.finish" && atom["payload"]["code"] == code
        ));
    }
    for edge in ["enter", "return"] {
        assert_eq!(
            atoms
                .iter()
                .filter(|atom| atom["source"]["edge"] == edge)
                .count(),
            10
        );
    }
    assert!(
        atoms
            .iter()
            .all(|atom| atom["context"].get("dynet.fault").is_none())
    );
    let text = serde_json::to_string(&atoms).unwrap();
    for private in [
        "private-instance",
        "private-label",
        "private-server",
        "private-uuid",
        "private-secret",
        "private-target",
        "private-bootstrap",
        "127.0.0.1",
    ] {
        assert!(!text.contains(private));
    }
}

#[test]
fn isolated() {
    let fixture = Fixture::new();
    let baseline = fixture.run("divert.unit");
    for text in [
        ENABLED,
        "[locus]\nenabled = 'private-setting'\n",
        "[locus]\nenabled = true\nendpoint = ''\n",
    ] {
        fixture.configure(text);
        let result = fixture.run("divert.unit");
        assert_eq!(result.status.code(), baseline.status.code());
        assert_eq!(result.stdout, baseline.stdout);
        assert!(!result.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("private-setting"));
        assert_eq!(fixture.run("spread").status.code(), Some(1));
        assert_eq!(fixture.run("identity").status.code(), Some(2));
    }
    let blocked = Fixture::new();
    blocked.configure(ENABLED);
    std::fs::write(blocked.seat().join("state"), "occupied").unwrap();
    let result = blocked.run("reach");
    assert_eq!(result.status.code(), Some(0));
    assert!(result.stdout.is_empty());
    assert!(!result.stderr.is_empty());
}

#[test]
fn parity() {
    let names = [
        "doctor",
        "up",
        "down",
        "standing",
        "resolve",
        "reach",
        "spread",
        "identity",
        "divert.unit",
    ];
    let fixture = Fixture::new();
    let off: Vec<_> = names.iter().map(|name| fixture.run(name)).collect();
    fixture.configure(ENABLED);
    for (name, baseline) in names.iter().zip(&off) {
        let on = fixture.run(name);
        assert_eq!(on.status.code(), baseline.status.code(), "{name}");
        assert_eq!(on.stdout, baseline.stdout, "{name}");
    }
}
