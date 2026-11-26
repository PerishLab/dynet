use dynet_api::{
    Loaded,
    control::{Payload, Private, Session},
    host::State,
    manifest,
};
use dynet_core::{Error, Instance};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Kind {
    Payload,
    Manifest,
}

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum, default_value_t = Kind::Payload)]
    kind: Kind,
}

pub fn read(path: &Path, instance: &Instance, state: &State) -> Result<Session, Error> {
    Session::open(path, instance, state, Private::current())
}

pub fn loaded(session: &Session) -> Result<&Loaded, Error> {
    session
        .initial
        .payload
        .as_ref()
        .map(|payload| &payload.loaded)
        .ok_or_else(|| Error::new("the signed configuration is revoked"))
}

pub fn check(errand: &Errand) -> Result<ExitCode, Error> {
    let bytes = load(&errand.input)?;
    match errand.kind {
        Kind::Payload => {
            let payload = Payload::check(&bytes)?;
            let manifest = payload.loaded.manifest();
            let label = manifest.generation().unwrap_or("none");
            println!(
                "ok: payload v1 (manifest.generation={label}, nodes={}, clusters={}, rules={}, upstream={}, retain={}s)",
                manifest.nodes().len(),
                manifest.clusters().len(),
                manifest.table().rules().len(),
                payload.upstream,
                payload.retain
            );
        }
        Kind::Manifest => {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("manifest input is not valid UTF-8"))?;
            let loaded = manifest::load(text)?;
            let manifest = loaded.manifest();
            let label = manifest.generation().unwrap_or("none");
            println!(
                "ok: manifest v1 (manifest.generation={label}, nodes={}, clusters={}, rules={})",
                manifest.nodes().len(),
                manifest.clusters().len(),
                manifest.table().rules().len()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn load(path: &Path) -> Result<Vec<u8>, Error> {
    if path == Path::new("-") {
        return bounded(std::io::stdin().lock());
    }
    let file = File::open(path)
        .map_err(|error| Error::new(format!("cannot open {}: {error}", path.display())))?;
    bounded(file)
}

fn bounded(source: impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    source
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::new(format!("cannot read input: {error}")))?;
    if bytes.len() > LIMIT {
        return Err(Error::new(format!(
            "input exceeds the {LIMIT}-byte payload bound"
        )));
    }
    Ok(bytes)
}
