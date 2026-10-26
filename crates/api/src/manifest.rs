use crate::outbound::{Front, Roster};
use dynet_core::{Declared, Error, Form, Manifest};
use std::collections::BTreeMap;
use std::path::Path;

const CIPHER: &str = "2022-blake3-aes-128-gcm";
const RENAMED: [(&str, &str); 4] = [
    ("tls.server_name", "servername"),
    ("tls.reality.public_key", "reality-opts.public-key"),
    ("tls.reality.short_id", "reality-opts.short-id"),
    ("method", "cipher"),
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Entry {
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct Loaded {
    manifest: Manifest,
    entries: Vec<Entry>,
}

impl Entry {
    pub fn shaped(fields: BTreeMap<String, String>) -> Self {
        Self { fields }
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.field(name) == Some("true")
    }
}

impl Loaded {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

pub fn read(path: &Path) -> Result<Loaded, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    load(&text)
}

pub fn load(text: &str) -> Result<Loaded, Error> {
    let manifest = Manifest::read(text)?;
    let entries: Vec<Entry> = manifest.nodes().iter().map(entry).collect();
    for cluster in manifest.clusters() {
        vouched(cluster.form(), &manifest, &entries)?;
    }
    Ok(Loaded { manifest, entries })
}

pub fn entry(node: &Declared) -> Entry {
    let mut fields: BTreeMap<String, String> = node
        .material()
        .iter()
        .map(|(name, value)| {
            let renamed = RENAMED
                .iter()
                .find(|(from, _)| from == name)
                .map_or(name.as_str(), |(_, to)| to);
            (renamed.to_string(), value.clone())
        })
        .collect();
    fields.insert("name".to_string(), node.label().get().to_string());
    let kind = match node.protocol() {
        "shadowsocks" => "ss",
        other => other,
    };
    fields.insert("type".to_string(), kind.to_string());
    Entry::shaped(fields)
}

fn vouched(form: &Form, manifest: &Manifest, entries: &[Entry]) -> Result<(), Error> {
    match form {
        Form::Pool(nodes) => nodes
            .iter()
            .try_for_each(|node| fronted(entries, node.label().get())),
        Form::Via(_, exit) => {
            exited(manifest, exit.get())?;
            Roster::new(entries, 0).exit(exit).map(drop)
        }
        Form::Alias(_) => Ok(()),
    }
}

fn fronted(entries: &[Entry], label: &str) -> Result<(), Error> {
    let entry = entries
        .iter()
        .find(|entry| entry.field("name") == Some(label))
        .ok_or_else(|| Error::new(format!("node {label} is not declared")))?;
    if !matches!(entry.field("type"), Some("vless" | "vmess")) {
        return Err(Error::new(format!(
            "node {label} sits in a pool but speaks {}, and a pool's nodes must speak vless or vmess",
            entry.field("type").unwrap_or_default()
        )));
    }
    let server = entry
        .field("server")
        .ok_or_else(|| Error::new(format!("node {label} carries no server")))?;
    Front::read(entry, server.to_string(), 0)
        .map(drop)
        .map_err(|error| Error::new(format!("node {label} cannot front: {error}")))
}

fn exited(manifest: &Manifest, label: &str) -> Result<(), Error> {
    let node = manifest
        .nodes()
        .iter()
        .find(|node| node.label().get() == label)
        .ok_or_else(|| Error::new(format!("exit {label} is not declared")))?;
    match (node.protocol(), node.field("method")) {
        ("shadowsocks", Some(CIPHER)) => Ok(()),
        _ => Err(Error::new(format!(
            "exit {label} must speak shadowsocks with {CIPHER}"
        ))),
    }
}
