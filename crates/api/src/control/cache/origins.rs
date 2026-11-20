use super::{Cache, LIMIT, custody, refused};
use crate::resolver::Chain;
use dynet_core::{Domain, Error, Name, Table};
use dynet_host::Private;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Witness {
    origin: Domain,
    cluster: Option<Name>,
    generation: String,
    until: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Origins {
    links: BTreeMap<Domain, Vec<Witness>>,
    #[serde(skip)]
    roots: BTreeMap<Domain, Vec<Domain>>,
}

pub(crate) struct Scope<'a> {
    pub root: &'a Domain,
    pub cluster: Option<&'a Name>,
    pub generation: &'a str,
    pub table: &'a Table,
}

impl Witness {
    fn matches(&self, scope: &Scope) -> bool {
        self.origin == *scope.root
            && self.cluster.as_ref() == scope.cluster
            && self.generation == scope.generation
    }
}

impl Origins {
    pub(super) fn open(root: &Path, private: &Private) -> Result<Self, Error> {
        let folder = root.join("origins");
        match std::fs::symlink_metadata(&folder) {
            Ok(_) => private.folder(&folder).map_err(custody)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return match private.read(&root.join("origins.enabled"), 1) {
                    Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
                    _ => Err(refused()),
                };
            }
            Err(error) => return Err(custody(error)),
        }
        let bytes = private
            .read(&folder.join("bindings"), LIMIT)
            .map_err(custody)?;
        let mut origins: Self = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        Self::mark(root, private)?;
        origins.index();
        Ok(origins)
    }
    fn mark(root: &Path, private: &Private) -> Result<(), Error> {
        let path = root.join("origins.enabled");
        match private.read(&path, 1) {
            Ok(bytes) if bytes == b"1" => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                private.create(&path, b"1").map_err(custody)
            }
            _ => Err(refused()),
        }
    }

    fn index(&mut self) {
        self.roots.clear();
        for (alias, owners) in &self.links {
            for owner in owners {
                let targets = self.roots.entry(owner.origin.clone()).or_default();
                if targets.last() != Some(alias) {
                    targets.push(alias.clone());
                }
            }
        }
    }

    fn remembers(&self, scope: &Scope, chain: &Chain, now: u64) -> bool {
        chain.links.iter().all(|(alias, until)| {
            *until <= now
                || self.links.get(alias).is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.matches(scope) && item.until >= *until)
                })
        })
    }

    fn observe(&mut self, scope: &Scope, chain: &Chain, now: u64) {
        self.links.retain(|_, witnesses| {
            witnesses.retain(|item| item.until > now);
            !witnesses.is_empty()
        });
        for (alias, until) in &chain.links {
            if *until <= now {
                continue;
            }
            let witnesses = self.links.entry(alias.clone()).or_default();
            let previous = witnesses.iter_mut().find(|item| item.matches(scope));
            if let Some(held) = previous {
                held.until = held.until.max(*until);
            } else {
                witnesses.push(Witness {
                    origin: scope.root.clone(),
                    cluster: scope.cluster.cloned(),
                    generation: scope.generation.into(),
                    until: *until,
                });
            }
        }
        self.index();
    }

    fn check(&self, scope: &Scope, chain: &Chain, now: u64) -> Result<(), String> {
        let previous = self
            .roots
            .get(scope.root)
            .into_iter()
            .flatten()
            .filter(|alias| {
                self.links.get(*alias).is_some_and(|owners| {
                    owners
                        .iter()
                        .any(|witness| witness.origin == *scope.root && witness.until > now)
                })
            });
        for alias in std::iter::once(scope.root)
            .chain(chain.links.keys())
            .chain(previous)
        {
            if let Some(owner) = scope.table.claimed(alias)
                && Some(owner) != scope.cluster
            {
                return Err(format!(
                    "alias {} explicitly belongs to {}",
                    alias.get(),
                    owner.get()
                ));
            }
            for held in self
                .links
                .get(alias)
                .into_iter()
                .flatten()
                .filter(|item| item.until > now)
            {
                if held.cluster.as_ref() != scope.cluster
                    || scope.table.claimed(&held.origin) != held.cluster.as_ref()
                {
                    let earlier = held.cluster.as_ref().map_or("direct", Name::get);
                    let current = scope.cluster.map_or("direct", Name::get);
                    return Err(format!(
                        "alias {} from {} ({earlier}) refused for {} ({current})",
                        alias.get(),
                        held.origin.get(),
                        scope.root.get()
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Cache {
    pub(crate) fn origins(
        &self,
        scope: &Scope,
        chain: &Chain,
        now: u64,
    ) -> Result<Result<(), String>, Error> {
        let mut held = self.origins.lock().map_err(|_| refused())?;
        if held.remembers(scope, chain, now) {
            return Ok(held.check(scope, chain, now));
        }
        let mut next = held.clone();
        next.observe(scope, chain, now);
        let decision = next.check(scope, chain, now);
        if *held != next {
            let bytes = serde_json::to_vec(&next).map_err(|_| refused())?;
            if bytes.len() > LIMIT {
                return Err(refused());
            }
            let folder = self.root.join("origins");
            self.private.folder(&folder).map_err(custody)?;
            self.private
                .replace(&folder.join("bindings"), &bytes)
                .map_err(custody)?;
            Origins::mark(&self.root, &self.private)?;
            *held = next;
        }
        Ok(decision)
    }
}
