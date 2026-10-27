use super::{paths, refused};
use crate::host::{Fragment, Held, MARKER, held, run};
use dynet_core::{Error, Instance};
use std::net::Ipv4Addr;

pub(super) struct Dns<'a>(pub(super) &'a Instance);

impl Dns<'_> {
    pub(super) fn present(&self) -> bool {
        held(&paths::path(self.0, "dns")).is_ok_and(|held| held == Held::Owned)
    }

    pub(super) fn raise(&self, seat: Ipv4Addr) -> Result<(), Error> {
        let path = paths::path(self.0, "dns");
        let records = match held(&path)? {
            Held::Foreign => {
                return Err(Error::new("the saved DNS state belongs to another owner"));
            }
            Held::Owned => self.read()?,
            Held::Absent => {
                let listed = run("/usr/sbin/networksetup", &["-listallnetworkservices"])?;
                let mut records = Vec::new();
                for service in listed
                    .lines()
                    .skip(1)
                    .filter(|line| !line.is_empty() && !line.starts_with('*'))
                {
                    let prior = current(service)?;
                    records.push((service.to_string(), prior));
                }
                let mut text = format!("{seat}\n");
                for (service, servers) in &records {
                    text.push_str(&format!("{service}\t{}\n", servers.join(",")));
                }
                Fragment::new(path, text).write()?;
                records
            }
        };
        for (service, prior) in &records {
            let now = current(service)?;
            if now != [seat.to_string()] && now != *prior {
                return Err(Error::new(format!(
                    "{service} DNS changed outside Dynet; startup refuses to overwrite it"
                )));
            }
        }
        for (service, _) in records {
            run(
                "/usr/sbin/networksetup",
                &["-setdnsservers", &service, &seat.to_string()],
            )?;
            if current(&service)? != [seat.to_string()] {
                return Err(Error::new(format!(
                    "{service} did not retain the Dynet resolver"
                )));
            }
        }
        flush()
    }

    pub(super) fn lower(&self) -> Result<(), Error> {
        let path = paths::path(self.0, "dns");
        if held(&path)? == Held::Absent {
            return Ok(());
        }
        let text = std::fs::read_to_string(&path).map_err(refused)?;
        let seat = text
            .lines()
            .nth(1)
            .ok_or_else(|| Error::new("the saved DNS state has no seat"))?;
        let records = self.read()?;
        for (service, prior) in &records {
            let now = current(service)?;
            if now != [seat.to_string()] && now != *prior {
                return Err(Error::new(format!(
                    "{service} DNS changed outside Dynet; it was left untouched"
                )));
            }
        }
        for (service, prior) in records {
            let mut arguments = vec!["-setdnsservers", &service];
            arguments.extend(prior.iter().map(String::as_str));
            if prior.is_empty() {
                arguments.push("Empty");
            }
            run("/usr/sbin/networksetup", &arguments)?;
        }
        Fragment::remove(&path)?;
        flush()
    }

    fn read(&self) -> Result<Vec<(String, Vec<String>)>, Error> {
        let path = paths::path(self.0, "dns");
        if held(&path)? != Held::Owned {
            return Err(Error::new("the saved DNS state is not owned"));
        }
        let text = std::fs::read_to_string(path).map_err(refused)?;
        if text.lines().next() != Some(MARKER) {
            return Err(Error::new("the saved DNS marker differs"));
        }
        text.lines()
            .skip(2)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let (service, prior) = line
                    .split_once('\t')
                    .ok_or_else(|| Error::new("the saved DNS record is malformed"))?;
                let servers = prior
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
                Ok((service.to_string(), servers))
            })
            .collect()
    }
}

fn current(service: &str) -> Result<Vec<String>, Error> {
    let output = run("/usr/sbin/networksetup", &["-getdnsservers", service])?;
    if output.starts_with("There aren't any DNS Servers") {
        return Ok(Vec::new());
    }
    output
        .lines()
        .map(|line| {
            line.parse::<std::net::IpAddr>()
                .map(|address| address.to_string())
                .map_err(|_| Error::new(format!("{service} returned an invalid DNS address")))
        })
        .collect()
}

fn flush() -> Result<(), Error> {
    run("/usr/bin/dscacheutil", &["-flushcache"])?;
    run("/usr/bin/killall", &["-HUP", "mDNSResponder"])?;
    Ok(())
}
