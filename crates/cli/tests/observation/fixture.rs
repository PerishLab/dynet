use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

pub(super) struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(fixture.seat()).unwrap();
        std::fs::write(fixture.root.path().join("subscription"), "proxies:\n  - {name: '🇭🇰 private-label', type: vmess, server: private-server.invalid, port: 443, uuid: private-uuid, udp: true}\n").unwrap();
        fixture
    }

    pub(super) fn seat(&self) -> PathBuf {
        self.root.path().join(".dynet")
    }

    pub(super) fn configure(&self, text: &str) {
        std::fs::write(self.seat().join("config.toml"), text).unwrap();
    }

    pub(super) fn run(&self, name: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dynet"));
        command
            .env_clear()
            .env("HOME", self.root.path())
            .env("USERPROFILE", self.root.path())
            .env("LOCALAPPDATA", self.root.path())
            .current_dir(self.root.path());
        match name {
            "doctor" | "up" | "down" | "standing" => {
                command.args(["--instance", "private-instance", name]);
            }
            "resolve" | "reach" | "spread" => {
                command.args([
                    name,
                    "--subscription",
                    "subscription",
                    "--cluster",
                    "hk",
                    "--count",
                    "0",
                ]);
                if name == "resolve" {
                    command.args(["--name", "private-target.invalid"]);
                }
            }
            "exit" => {
                command.args([
                    "exit",
                    "--seat",
                    "private-seat",
                    "--secret",
                    "private-secret",
                    "--target",
                    "private-target.invalid",
                ]);
            }
            "divert.unit" => {
                command.args([
                    "divert",
                    "--subscription",
                    "private-subscription",
                    "--clusters",
                    "private-clusters",
                    "--table",
                    "private-table",
                    "--unit",
                ]);
            }
            _ => panic!("unknown fixture command"),
        }
        command.output().unwrap()
    }

    pub(super) fn records(&self) -> Vec<Value> {
        let mut atoms = Vec::new();
        for entry in std::fs::read_dir(self.seat().join("state/locus")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if name == "active.jsonl" || name.starts_with("sealed-") {
                let text = std::fs::read_to_string(path).unwrap();
                atoms.extend(
                    text.lines()
                        .map(|line| serde_json::from_str::<Value>(line).unwrap()),
                );
            }
        }
        atoms
    }
}
