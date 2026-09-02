use super::fragment::Fragment;
use super::{attempt, run};
use dynet_core::{Error, Instance};
use std::path::PathBuf;

const NUMBER: &str = "178";

pub struct Route;

impl Route {
    pub fn registry(instance: &Instance) -> PathBuf {
        PathBuf::from(format!("/etc/iproute2/rt_tables.d/{}.conf", instance.get()))
    }

    pub fn declared(instance: &Instance) -> bool {
        let priority = instance.priority().to_string();
        run("ip", &["rule", "show", "priority", &priority])
            .is_ok_and(|text| !text.trim().is_empty())
    }

    pub fn create(instance: &Instance, claim: &str) -> Result<(), Error> {
        let name = instance.get();
        let priority = instance.priority().to_string();
        Fragment::new(Self::registry(instance), format!("{NUMBER}\t{name}\n")).write()?;
        run("ip", &["route", "add", claim, "dev", name, "table", name])?;
        run(
            "ip",
            &[
                "rule", "add", "to", claim, "priority", &priority, "table", name,
            ],
        )?;
        Ok(())
    }

    pub fn destroy(instance: &Instance) -> Result<bool, Error> {
        let name = instance.get();
        let priority = instance.priority().to_string();
        let mut removed = false;
        while Self::declared(instance) {
            run("ip", &["rule", "del", "priority", &priority])?;
            removed = true;
        }
        if attempt("ip", &["route", "show", "table", name]) {
            attempt("ip", &["route", "flush", "table", name]);
        }
        removed |= Fragment::remove(&Self::registry(instance))?;
        Ok(removed)
    }
}
