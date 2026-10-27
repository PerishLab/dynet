use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

const MANIFEST: &str = r#"{"version": 1, "nodes": [{"label": "private-label", "protocol": "vmess", "server": "private-server.invalid", "port": 443, "uuid": "6f1c2a3b-0000-4000-8000-000000000001", "udp": true}], "clusters": [{"name": "hk", "nodes": ["private-label"]}], "rules": [], "fallback": "direct"}"#;

pub(super) struct Fixture {
    _home: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let mut builder = tempfile::Builder::new();
        builder.prefix("dynet-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        #[cfg(windows)]
        let home = builder
            .tempdir_in(dynet_host::windows::data().unwrap())
            .unwrap();
        #[cfg(unix)]
        let home = builder.tempdir().unwrap();
        let root = home.path().canonicalize().unwrap().join("private");
        dynet_api::control::Private::current()
            .folder(&root)
            .unwrap();
        let fixture = Self { _home: home, root };
        std::fs::create_dir(fixture.seat()).unwrap();
        fixture.enroll();
        fixture
    }

    fn enroll(&self) {
        use secrets::ExposeSecret;
        let private = dynet_api::control::Private::current();
        let root = self.root.as_path().canonicalize().unwrap();
        private.folder(&root.join("state")).unwrap();
        let identity = secrets::Identity::generate();
        let signer = secrets::Signer::generate().unwrap();
        let target = secrets::Target::new("fixture", "routing", "epoch1").unwrap();
        let payload = serde_json::json!({"version":1,"manifest":serde_json::from_str::<Value>(MANIFEST).unwrap(),"upstream":"1.1.1.1","retain":86400});
        let envelope = secrets::Envelope::seal(
            target.clone(),
            1,
            &serde_json::to_vec(&payload).unwrap(),
            (&identity.recipient(), &signer),
        )
        .unwrap();
        private
            .create(
                &root.join("identity"),
                identity.export().expose_secret().as_bytes(),
            )
            .unwrap();
        private.create(&root.join("envelope"), &envelope).unwrap();
        let bootstrap = serde_json::json!({"version":1,"target":target,"identityRef":root.join("identity"),"publisher":signer.publisher().unwrap().to_string(),"minimum":1,"source":{"kind":"file","path":root.join("envelope")}});
        private
            .create(
                &root.join("bootstrap"),
                &serde_json::to_vec(&bootstrap).unwrap(),
            )
            .unwrap();
    }

    pub(super) fn seat(&self) -> PathBuf {
        self.root
            .as_path()
            .join(if cfg!(windows) { "dynet" } else { ".dynet" })
    }

    pub(super) fn configure(&self, text: &str) {
        std::fs::write(self.seat().join("config.toml"), text).unwrap();
    }

    pub(super) fn run(&self, name: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dynet"));
        command
            .env_clear()
            .env("HOME", self.root.as_path())
            .env("USERPROFILE", self.root.as_path())
            .env("LOCALAPPDATA", self.root.as_path())
            .env(
                "DYNET_STATE",
                self.root.as_path().canonicalize().unwrap().join("state"),
            )
            .current_dir(self.root.as_path());
        match name {
            "doctor" | "up" | "down" | "standing" => {
                command.args(["--instance", "private-instance", name]);
            }
            "resolve" | "reach" | "spread" => {
                command.args([
                    name,
                    "--bootstrap",
                    self.root
                        .as_path()
                        .canonicalize()
                        .unwrap()
                        .join("bootstrap")
                        .to_str()
                        .unwrap(),
                    "--cluster",
                    "hk",
                    "--count",
                    "0",
                ]);
                if name == "resolve" {
                    command.args(["--name", "private-target.invalid"]);
                }
            }
            "identity" => {
                command.args(["identity", "--output", "private-secret-output"]);
            }
            "divert.unit" => {
                command.args(["divert", "--bootstrap", "private-bootstrap", "--unit"]);
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
