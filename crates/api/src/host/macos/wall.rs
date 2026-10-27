use super::{dns::Dns, paths, refused};
use crate::host::{Fragment, Held, attempt, feed, held, run};
use dynet_core::{Error, Instance, Range};
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, UdpSocket};
use std::process::Command;

pub struct Veil;

impl Veil {
    pub fn raised(instance: &Instance) -> bool {
        Self::walled(instance) || Dns(instance).present()
    }

    pub fn walled(instance: &Instance) -> bool {
        let anchor = Wall(instance).anchor();
        let active =
            run("/sbin/pfctl", &["-s", "info"]).is_ok_and(|text| text.contains("Status: Enabled"));
        let hooked = run("/sbin/pfctl", &["-sr"]).is_ok_and(|text| {
            text.lines()
                .any(|line| line.starts_with("anchor \"com.apple/*\""))
        });
        let ruled = run("/sbin/pfctl", &["-a", &anchor, "-sr"])
            .is_ok_and(|text| text.contains("<captured>"));
        active && hooked && ruled
    }

    pub fn attended(seat: Ipv4Addr, port: u16) -> bool {
        UdpSocket::bind((seat, port))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::AddrInUse)
    }

    pub fn raise(instance: &Instance, seat: Ipv4Addr) -> Result<(), Error> {
        let root = run("/sbin/pfctl", &["-sr"])?;
        if !root
            .lines()
            .any(|line| line.starts_with("anchor \"com.apple/*\""))
        {
            return Err(Error::new(
                "PF has no com.apple wildcard anchor; the root rules were left untouched",
            ));
        }
        let face = dynet_host::macos::name(instance.get());
        let rules = rules(&face, seat);
        let anchor = Wall(instance).anchor();
        feed("/sbin/pfctl", &["-n", "-a", &anchor, "-f", "-"], &rules)?;
        feed("/sbin/pfctl", &["-a", &anchor, "-f", "-"], &rules)?;
        Wall(instance).enable()?;
        if !Self::walled(instance) {
            return Err(Error::new("PF did not retain the capture protection"));
        }
        Dns(instance).raise(seat)
    }

    pub fn lower(instance: &Instance) -> Result<bool, Error> {
        let present = Self::raised(instance);
        Dns(instance).lower()?;
        let anchor = Wall(instance).anchor();
        feed("/sbin/pfctl", &["-a", &anchor, "-f", "-"], "")?;
        let _ = attempt(
            "/sbin/pfctl",
            &["-a", &anchor, "-t", "captured", "-T", "kill"],
        );
        let path = paths::path(instance, "pf");
        if held(&path)? == Held::Owned {
            let text = std::fs::read_to_string(&path).map_err(refused)?;
            if let Some(token) = text
                .lines()
                .find(|line| line.bytes().all(|byte| byte.is_ascii_digit()) && !line.is_empty())
            {
                run("/sbin/pfctl", &["-X", token])?;
            }
            Fragment::remove(&path)?;
        }
        Ok(present)
    }
}

pub(super) struct Wall<'a>(pub(super) &'a Instance);

impl Wall<'_> {
    pub(super) fn anchor(&self) -> String {
        format!("com.apple/dynet-{}", self.0.get())
    }

    pub(super) fn shown(&self) -> Result<BTreeSet<Range>, Error> {
        let text = run(
            "/sbin/pfctl",
            &["-a", &self.anchor(), "-t", "captured", "-T", "show"],
        )?;
        text.split_whitespace()
            .map(|line| match line.contains('/') {
                true => Range::read(line),
                false => line
                    .parse()
                    .map(Range::host)
                    .map_err(|_| Error::new("PF returned an invalid address")),
            })
            .collect()
    }

    pub(super) fn settle(&self, wanted: &BTreeSet<Range>) -> Result<(), Error> {
        self.attest()?;
        let lines: String = wanted
            .iter()
            .map(|range| format!("{}/{}\n", range.base(), range.prefix()))
            .collect();
        feed(
            "/sbin/pfctl",
            &[
                "-a",
                &self.anchor(),
                "-t",
                "captured",
                "-T",
                "replace",
                "-f",
                "-",
            ],
            &lines,
        )?;
        if self.shown()? != *wanted {
            return Err(Error::new("PF differs from the requested capture"));
        }
        Ok(())
    }

    pub(super) fn hold(&self, address: std::net::IpAddr) -> Result<(), Error> {
        self.attest()?;
        run(
            "/sbin/pfctl",
            &[
                "-a",
                &self.anchor(),
                "-t",
                "captured",
                "-T",
                "add",
                &address.to_string(),
            ],
        )?;
        if !self.shown()?.iter().any(|range| range.holds(address)) {
            return Err(Error::new("PF did not protect the issued address"));
        }
        Ok(())
    }

    fn attest(&self) -> Result<(), Error> {
        match Veil::walled(self.0) {
            true => Ok(()),
            false => Err(Error::new(
                "PF capture protection is not active and reachable",
            )),
        }
    }

    fn enable(&self) -> Result<(), Error> {
        let path = paths::path(self.0, "pf");
        let enabled = run("/sbin/pfctl", &["-s", "info"])?.contains("Status: Enabled");
        if enabled && held(&path)? == Held::Owned {
            return Ok(());
        }
        let output = Command::new("/sbin/pfctl")
            .arg("-E")
            .output()
            .map_err(refused)?;
        if !output.status.success() {
            return Err(Error::new("PF refused to enable capture protection"));
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let token = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("Token : "))
            .ok_or_else(|| Error::new("PF enabled without returning its reference token"))?;
        Fragment::new(path, token).write()
    }
}

fn rules(face: &str, seat: Ipv4Addr) -> String {
    format!(
        "table <captured> persist\npass out quick on lo0 all no state\npass out quick on {face} all no state\nblock return out quick inet to {seat}\npass out quick proto {{ tcp udp }} all user root no state\nblock return out quick inet to <captured>\nblock return out quick proto {{ tcp udp }} to any port 53\n"
    )
}
