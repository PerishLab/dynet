use super::{attempt, run};
use dynet_core::{Error, Instance, Span};
use std::net::Ipv4Addr;

pub struct Veil;

fn capture(seat: Ipv4Addr, port: u16, from: &str) -> String {
    format!("    {from}meta l4proto {{ tcp, udp }} th dport 53 dnat ip to {seat}:{port}\n")
}

impl Veil {
    pub fn raised(instance: &Instance) -> bool {
        attempt("nft", &["list", "table", "inet", instance.get()])
    }

    pub fn raise(
        instance: &Instance,
        ground: (Span, u16),
        sources: &[String],
    ) -> Result<(), Error> {
        let (span, port) = ground;
        let name = instance.get();
        let mark = instance.mark();
        let seat = span.seat();
        let mut rules = format!(
            "table inet {name} {{\n  chain output {{\n    type nat hook output priority -100; policy accept;\n    meta mark {mark} accept\n{}  }}\n",
            capture(seat, port, "")
        );
        if !sources.is_empty() {
            let from = format!("ip saddr {{ {} }} ", sources.join(", "));
            rules.push_str(&format!(
                "  chain prerouting {{\n    type nat hook prerouting priority -100; policy accept;\n{}  }}\n",
                capture(seat, port, &from)
            ));
        }
        rules.push_str("}\n");
        write(&rules)
    }

    pub fn lower(instance: &Instance) -> Result<bool, Error> {
        if !Self::raised(instance) {
            return Ok(false);
        }
        run("nft", &["delete", "table", "inet", instance.get()])?;
        Ok(true)
    }

    pub fn strays() -> Vec<String> {
        let Ok(listing) = run("nft", &["list", "tables", "inet"]) else {
            return Vec::new();
        };
        listing
            .lines()
            .filter_map(|line| line.split_whitespace().nth(2))
            .filter(|name| name.starts_with("dynet"))
            .map(str::to_string)
            .collect()
    }
}

fn write(rules: &str) -> Result<(), Error> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::new(format!("cannot run nft: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| Error::new("nft refused its input"))?
        .write_all(rules.as_bytes())
        .map_err(|error| Error::new(format!("cannot feed nft: {error}")))?;
    let output = child
        .wait_with_output()
        .map_err(|error| Error::new(format!("nft did not finish: {error}")))?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "nft refused the ruleset: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}
