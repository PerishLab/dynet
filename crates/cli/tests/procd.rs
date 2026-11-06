#![cfg(target_os = "linux")]

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn render(bootstrap: &str, state: bool, local: bool, extra: &[&str]) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dynet"));
    command
        .env("HOME", home.path())
        .env("DYNET_LOCAL", local.to_string())
        .env("DYNET_SOURCES", "192.168.1.22/32")
        .env("DYNET_TRANSIT", "1053")
        .env_remove("DYNET_STATE")
        .args([
            "--instance",
            "dynetpilot",
            "--port",
            "5353",
            "--span",
            "198.51.100.0/24",
        ]);
    if state {
        command.args(["--state", "/private/state ' $(false)"]);
    }
    command
        .args(["divert", "--unit", "--procd", "--bootstrap", bootstrap])
        .args(extra)
        .output()
        .unwrap()
}

fn shell(text: &[u8], action: &str) -> Output {
    let mut child = Command::new("/bin/sh")
        .args(["-s", "--", action])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(text).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn protocol() {
    let bootstrap = "/private/bootstrap ' $(exit 41)\nnext";
    let output = render(bootstrap, true, false, &[]);
    assert!(output.status.success(), "{:?}", output);
    let mut text = b"procd_open_instance() { printf 'instance\\000%s\\000' \"$1\"; }\nprocd_set_param() { printf '%s\\000' \"$@\"; printf '\\000'; }\nprocd_close_instance() { printf 'closed\\000'; }\n".to_vec();
    text.extend(output.stdout);
    text.extend(b"\nstart_service\n");
    let ran = shell(&text, "start");
    assert!(ran.status.success(), "{:?}", ran);
    let fields = ran.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
    let values = fields
        .iter()
        .map(|value| std::str::from_utf8(value).unwrap())
        .collect::<Vec<_>>();
    let command = values.iter().position(|value| *value == "command").unwrap();
    assert_eq!(
        &values[command + 1..command + 17],
        &[
            env!("CARGO_BIN_EXE_dynet"),
            "--instance",
            "dynetpilot",
            "--port",
            "5353",
            "--span",
            "198.51.100.0/24",
            "--state",
            "/private/state ' $(false)",
            "divert",
            "--bootstrap",
            bootstrap,
            "--claim",
            "0.0.0.0/0",
            "--seconds",
            "0"
        ]
    );
    assert!(values.contains(&"DYNET_LOCAL=false"));
    assert!(values.contains(&"DYNET_SOURCES=192.168.1.22/32"));
    assert!(values.contains(&"DYNET_TRANSIT=1053"));
    assert!(values.contains(&"as=536870912 536870912"));
    assert!(values.contains(&"nofile=512 512"));
    assert_eq!(values.last(), Some(&""));
    assert!(values.contains(&"closed"));
}

#[test]
fn retained() {
    let output = render("/private/bootstrap", true, false, &[]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("stop_service"));
    assert!(!text.contains("service_stopped"));
    assert!(!text.contains("'up'"));
    assert!(!text.contains("'down'"));
    assert!(!text.contains("START="));
    assert!(!text.contains("STOP="));
    assert!(text.contains("respawn 3600 5 5"));
    assert!(text.contains("term_timeout 10"));
    assert!(text.contains("procd_send_signal"));
    assert!(text.contains("stdout 0"));
    assert!(text.contains("stderr 1"));
    assert!(shell(text.as_bytes(), "parse").status.success());
}

#[test]
fn refused() {
    for output in [
        render("/private/bootstrap", false, false, &[]),
        render("/private/bootstrap", true, true, &[]),
        render("relative", true, false, &[]),
        render("/private/bootstrap", true, false, &["--bare"]),
        render("/private/bootstrap", true, false, &["--service"]),
    ] {
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn reload() {
    let output = render("/private/bootstrap", true, false, &[]);
    assert!(output.status.success());
    let mut text = output.stdout;
    text.extend(b"\nprocd_send_signal() { printf '%s\\000' \"$@\"; }\ninitscript='/etc/init.d/dynet-pilot'\nreload_service\n");
    let output = shell(&text, "reload");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"dynet-pilot\0dynetpilot\0HUP\0");
}
