mod cache;
mod custody;
mod projection;
mod recovery;

use super::{Cache, Decoder};
use dynet_host::Private;
use secrets::{Envelope, Identity, Reader, Signer, Target};
use std::path::PathBuf;

struct Fixture {
    _home: tempfile::TempDir,
    root: PathBuf,
    signer: Signer,
    identity: Identity,
    target: Target,
}

impl Fixture {
    fn new() -> Self {
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
        Private::current().folder(&root).unwrap();
        Self {
            _home: home,
            root,
            signer: Signer::generate().unwrap(),
            identity: Identity::generate(),
            target: Target::new("dynet0", "routing", "enrollment1").unwrap(),
        }
    }

    fn bootstrap(&self, content: &[u8]) -> PathBuf {
        use secrets::ExposeSecret;
        let private = Private::current();
        private
            .create(
                &self.root.join("identity"),
                self.identity.export().expose_secret().as_bytes(),
            )
            .unwrap();
        private
            .create(&self.root.join("envelope"), &self.seal(1, content))
            .unwrap();
        let bootstrap = serde_json::json!({"version":1,"target":self.target,"identityRef":self.root.join("identity"),"publisher":self.signer.publisher().unwrap().to_string(),"minimum":1,"source":{"kind":"file","path":self.root.join("envelope")}});
        private
            .create(
                &self.root.join("bootstrap"),
                &serde_json::to_vec(&bootstrap).unwrap(),
            )
            .unwrap();
        self.root.join("bootstrap")
    }

    fn decoder(&self) -> Decoder {
        let reader = Reader::new(self.target.clone(), self.signer.publisher().unwrap(), 1).unwrap();
        use secrets::ExposeSecret;
        let identity = Identity::read(self.identity.export().expose_secret()).unwrap();
        Decoder::new(reader, identity)
    }

    fn cache(&self) -> Cache {
        Cache::open(
            &self.root.join("cache"),
            "trust".to_owned(),
            Private::current(),
        )
        .unwrap()
    }

    fn seal(&self, generation: u64, content: &[u8]) -> Vec<u8> {
        Envelope::seal(
            self.target.clone(),
            generation,
            content,
            (&self.identity.recipient(), &self.signer),
        )
        .unwrap()
    }

    fn content(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "upstream": "1.1.1.1", "retain": 86400,
            "manifest": {"version":1,"nodes":[],"clusters":[],"rules":[],"fallback":"direct"}
        }))
        .unwrap()
    }
}
