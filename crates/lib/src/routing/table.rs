use super::domain::Domain;
use super::reserved::reserved;
use super::rule::{Rule, Subject};
use crate::Name;
use crate::error::Error;
use std::collections::BTreeSet;
use std::net::IpAddr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    rules: Vec<Rule>,
    fallback: Option<Name>,
}

impl Table {
    pub fn new(rules: Vec<Rule>, fallback: Option<Name>) -> Result<Self, Error> {
        let mut seen = BTreeSet::new();
        for rule in &rules {
            let told = spoken(rule.subject());
            if !seen.insert(told.clone()) {
                return Err(Error::new(format!(
                    "{told} appears in more than one rule; one name or prefix belongs to one rule"
                )));
            }
            spare(rule.subject())?;
        }
        Ok(Self { rules, fallback })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn fallback(&self) -> Option<&Name> {
        self.fallback.as_ref()
    }

    pub fn claimed(&self, domain: &Domain) -> Option<&Name> {
        self.rules
            .iter()
            .find(|rule| rule.named(domain))
            .map(Rule::cluster)
    }

    pub fn held(&self, address: IpAddr) -> Option<&Name> {
        self.rules
            .iter()
            .find(|rule| rule.addressed(address))
            .map(Rule::cluster)
    }
}

fn spoken(subject: &Subject) -> String {
    match subject {
        Subject::Exact(domain) | Subject::Suffix(domain) => domain.get().to_string(),
        Subject::Holds(range) => format!("{}/{}", range.base(), range.prefix()),
    }
}

fn spare(subject: &Subject) -> Result<(), Error> {
    let Subject::Holds(range) = subject else {
        return Ok(());
    };
    match reserved().into_iter().find(|kept| kept.overlaps(*range)) {
        Some(kept) => Err(Error::new(format!(
            "holds {}/{} overlaps the reserved {}/{}, which is never captured",
            range.base(),
            range.prefix(),
            kept.base(),
            kept.prefix()
        ))),
        None => Ok(()),
    }
}
