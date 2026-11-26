use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Output, Stdio};

const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const SECRET: &str = "AAECAwQFBgcICQoLDA0ODw==";

fn manifest(label: Value) -> Value {
    let reality = json!({"public_key": KEY, "short_id": "0a1b"});
    let tls = json!({"server_name": "cdn.example", "reality": reality});
    json!({
        "version": 1,
        "generation": label,
        "nodes": [
            {"label": "zx/tunnel", "protocol": "vless", "server": "192.0.2.10", "port": 443,
             "uuid": "6f1c2a3b-0000-4000-8000-000000000001", "flow": "xtls-rprx-vision", "tls": tls},
            {"label": "lisa/exit", "protocol": "shadowsocks", "server": "192.0.2.20", "port": 8388,
             "method": "2022-blake3-aes-128-gcm", "password": SECRET}
        ],
        "clusters": [
            {"name": "tunnel", "nodes": ["zx/tunnel"]},
            {"name": "veiled", "via": "tunnel", "exit": "lisa/exit"}
        ],
        "rules": [{"suffix": "example.com", "cluster": "veiled"}],
        "fallback": "direct"
    })
}

fn payload(label: Value) -> Value {
    json!({
        "version": 1,
        "upstream": "1.1.1.1",
        "retain": 86400,
        "manifest": manifest(label)
    })
}

fn run(args: &[&str], stdin: Option<&[u8]>) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dynet"));
    command
        .env("HOME", home.path())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("payload.json");
    std::fs::write(&path, payload(json!("pixel-full")).to_string()).unwrap();
    let output = run(&["check", "--input", path.to_str().unwrap()], None);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("ok: payload v1"), "{stdout}");
    assert!(
        stdout.contains("manifest.generation=pixel-full"),
        "{stdout}"
    );
    assert!(!stdout.contains(KEY), "{stdout}");
    assert!(!stdout.contains(SECRET), "{stdout}");
    assert!(!stdout.contains("6f1c2a3b"), "{stdout}");
}

#[test]
fn stdin() {
    let good = payload(json!("probe")).to_string();
    let output = run(&["check", "--input", "-"], Some(good.as_bytes()));
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("manifest.generation=probe"), "{stdout}");

    let bad = payload(json!(25)).to_string();
    let output = run(&["check", "--input", "-"], Some(bad.as_bytes()));
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("manifest.generation"), "{stderr}");
    assert!(stderr.contains("not a number"), "{stderr}");
    assert!(stderr.contains("--generation"), "{stderr}");
}

#[test]
fn raw() {
    let temp = tempfile::tempdir().unwrap();
    let good = temp.path().join("manifest.json");
    std::fs::write(&good, manifest(json!("probe")).to_string()).unwrap();
    let output = run(
        &[
            "check",
            "--kind",
            "manifest",
            "--input",
            good.to_str().unwrap(),
        ],
        None,
    );
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("ok: manifest v1"), "{stdout}");
    assert!(stdout.contains("manifest.generation=probe"), "{stdout}");

    let bad = temp.path().join("bad.json");
    std::fs::write(&bad, manifest(json!(25)).to_string()).unwrap();
    let output = run(
        &[
            "check",
            "--kind",
            "manifest",
            "--input",
            bad.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("manifest.generation"), "{stderr}");
    assert!(stderr.contains("--generation"), "{stderr}");
}
