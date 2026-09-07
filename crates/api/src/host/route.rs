use super::fragment::Fragment;
use super::{attempt, run};
use dynet_core::{Error, Instance, Span};
use std::path::PathBuf;

const WHOLE: &str = "/0";

pub struct Route;

impl Route {
    pub fn registry(instance: &Instance) -> PathBuf {
        PathBuf::from(format!("/etc/iproute2/rt_tables.d/{}.conf", instance.get()))
    }

    pub fn declared(instance: &Instance) -> bool {
        Self::ruled(instance.priority()) || Self::ruled(instance.priority() - 1)
    }

    fn ruled(priority: u32) -> bool {
        let priority = priority.to_string();
        run("ip", &["rule", "show", "priority", &priority])
            .is_ok_and(|text| !text.trim().is_empty())
    }

    pub fn create(instance: &Instance, span: Span, told: (&str, &[String])) -> Result<(), Error> {
        let (claim, sources) = told;
        let name = instance.get();
        let priority = instance.priority();
        let number = instance.table();
        Fragment::new(Self::registry(instance), format!("{number}\t{name}\n")).write()?;
        if !claim.ends_with(WHOLE) {
            run("ip", &["route", "add", claim, "dev", name, "table", name])?;
        }
        run(
            "ip",
            &["route", "add", &span.get(), "dev", name, "table", name],
        )?;
        let spared = (priority - 1).to_string();
        let mark = instance.mark().to_string();
        run(
            "ip",
            &[
                "rule", "add", "fwmark", &mark, "priority", &spared, "table", "main",
            ],
        )?;
        Self::divert(instance, claim, "lo")?;
        for source in sources {
            Self::spare(instance, claim, source)?;
        }
        Ok(())
    }

    fn divert(instance: &Instance, claim: &str, arriving: &str) -> Result<(), Error> {
        let priority = instance.priority().to_string();
        run(
            "ip",
            &[
                "rule",
                "add",
                "iif",
                arriving,
                "to",
                claim,
                "priority",
                &priority,
                "table",
                instance.get(),
            ],
        )
        .map(|_| ())
    }

    fn spare(instance: &Instance, claim: &str, source: &str) -> Result<(), Error> {
        let priority = instance.priority().to_string();
        run(
            "ip",
            &[
                "rule",
                "add",
                "from",
                source,
                "to",
                claim,
                "priority",
                &priority,
                "table",
                instance.get(),
            ],
        )
        .map(|_| ())
    }

    pub fn hold(instance: &Instance, address: &str) -> bool {
        let name = instance.get();
        let route = format!("{address}/32");
        attempt("ip", &["route", "add", &route, "dev", name, "table", name])
    }

    pub fn release(instance: &Instance, address: &str) -> bool {
        let name = instance.get();
        let route = format!("{address}/32");
        attempt("ip", &["route", "del", &route, "dev", name, "table", name])
    }

    pub fn destroy(instance: &Instance) -> Result<bool, Error> {
        let name = instance.get();
        let priority = instance.priority().to_string();
        let mut removed = false;
        let spared = (instance.priority() - 1).to_string();
        while Self::ruled(instance.priority()) {
            run("ip", &["rule", "del", "priority", &priority])?;
            removed = true;
        }
        while Self::ruled(instance.priority() - 1) {
            run("ip", &["rule", "del", "priority", &spared])?;
            removed = true;
        }
        if attempt("ip", &["route", "show", "table", name]) {
            attempt("ip", &["route", "flush", "table", name]);
        }
        removed |= Fragment::remove(&Self::registry(instance))?;
        Ok(removed)
    }
}
