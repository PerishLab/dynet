use super::{attempt, run};
use dynet_core::{Error, Instance};

const ROOM: &str = "16384";

pub struct Link;

impl Link {
    pub fn present(instance: &Instance) -> bool {
        attempt("ip", &["link", "show", instance.get()])
    }

    pub fn tunnel(instance: &Instance) -> Result<bool, Error> {
        if !Self::present(instance) {
            return Ok(false);
        }
        let detail = run("ip", &["-details", "link", "show", instance.get()])?;
        Ok(detail.contains("tun "))
    }

    pub fn create(instance: &Instance) -> Result<(), Error> {
        let name = instance.get();
        run("ip", &["tuntap", "add", "dev", name, "mode", "tun"])?;
        run("ip", &["link", "set", name, "mtu", ROOM])?;
        run("ip", &["link", "set", name, "up"])?;
        Ok(())
    }

    pub fn destroy(instance: &Instance) -> Result<bool, Error> {
        if !Self::present(instance) {
            return Ok(false);
        }
        if !Self::tunnel(instance)? {
            return Err(Error::new(format!(
                "{} exists but is not a tunnel device and was left alone",
                instance.get()
            )));
        }
        run("ip", &["link", "del", instance.get()])?;
        Ok(true)
    }

    pub fn strays() -> Vec<String> {
        let Ok(listing) = run("ip", &["-br", "link", "show"]) else {
            return Vec::new();
        };
        listing
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| name.starts_with("dynet"))
            .map(str::to_string)
            .collect()
    }
}
