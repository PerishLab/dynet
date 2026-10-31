#![cfg(target_os = "linux")]

use std::process::{Command, Output};

fn unit(local: Option<&str>, sources: &str) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dynet"));
    command
        .env("HOME", home.path())
        .env("DYNET_SOURCES", sources)
        .env_remove("DYNET_LOCAL")
        .args(["divert", "--unit", "--bootstrap", "/private/bootstrap"]);
    if let Some(value) = local {
        command.env("DYNET_LOCAL", value);
    }
    command.output().unwrap()
}

#[test]
fn default() {
    let implicit = unit(None, "");
    let explicit = unit(Some("true"), "");
    assert!(implicit.status.success());
    assert_eq!(implicit.stdout, explicit.stdout);
    let text = String::from_utf8(implicit.stdout).unwrap();
    assert!(!text.contains("DYNET_LOCAL=false"));
}

#[test]
fn forwarded() {
    let output = unit(Some("false"), "192.168.1.22/32");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Environment=DYNET_LOCAL=false"));
    assert!(text.contains("Environment=\"DYNET_SOURCES=192.168.1.22/32\""));
    assert!(text.contains("ExecStartPre=/usr/local/bin/dynet --instance dynet0 up"));
}

#[test]
fn empty() {
    for sources in ["", " , , "] {
        let output = unit(Some("false"), sources);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(text.contains("forwarded-only capture requires declared sources"));
    }
}
