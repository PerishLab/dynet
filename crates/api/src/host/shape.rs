use dynet_core::Error;
use std::fs;
use std::path::Path;

const STUB: &str = "/run/systemd/resolve/stub-resolv.conf";
const RESOLV: &str = "/etc/resolv.conf";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Shape {
    Resolved { upstream: bool },
    Plain,
    Missing,
}

impl Shape {
    pub fn ownable(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    pub fn explain(&self) -> String {
        match self {
            Self::Resolved { upstream: true } => {
                "systemd-resolved manages resolution and carries an upstream".to_string()
            }
            Self::Resolved { upstream: false } => concat!(
                "systemd-resolved manages resolution but names no upstream, so every ",
                "lookup already fails; on a host whose interface is configured by ifupdown ",
                "the upstream lives in dns-nameservers, which resolved does not read. ",
                "Repair: write DNS= and Domains=~. into a drop-in under ",
                "/etc/systemd/resolved.conf.d before starting dynet"
            )
            .to_string(),
            Self::Plain => concat!(
                "/etc/resolv.conf is a plain file no service manages, so dynet has no ",
                "fragment of its own to own and would have to overwrite the operator's file. ",
                "Repair: adopt systemd-resolved, or point resolution at a manager that reads ",
                "a drop-in directory"
            )
            .to_string(),
            Self::Missing => concat!(
                "resolution could not be read at all, which is not the same as being healthy. ",
                "Repair: make /etc/resolv.conf readable before asking dynet to judge it"
            )
            .to_string(),
        }
    }
}

pub fn survey() -> Result<Shape, Error> {
    let path = Path::new(RESOLV);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(Shape::Missing);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(Shape::Plain);
    }
    let target = fs::read_link(path)
        .map_err(|error| Error::new(format!("cannot read {RESOLV}: {error}")))?;
    if !target.ends_with("stub-resolv.conf") && !target.ends_with("resolv.conf") {
        return Ok(Shape::Plain);
    }
    if !Path::new(STUB).exists() {
        return Ok(Shape::Plain);
    }
    Ok(Shape::Resolved {
        upstream: upstream(),
    })
}

fn upstream() -> bool {
    let Ok(text) = fs::read_to_string(STUB) else {
        return false;
    };
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver "))
        .any(|value| !value.trim().is_empty())
}
