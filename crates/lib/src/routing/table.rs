use super::domain::Domain;
use super::rule::Rule;
use crate::cluster::Name;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ground {
    Named,
    Addressed,
    Default,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    cluster: Name,
    ground: Ground,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    rules: Vec<Rule>,
    fallback: Name,
}

impl Decision {
    pub fn new(cluster: Name, ground: Ground) -> Self {
        Self { cluster, ground }
    }

    pub fn cluster(&self) -> &Name {
        &self.cluster
    }

    pub fn ground(&self) -> Ground {
        self.ground
    }
}

impl Table {
    pub fn new(rules: Vec<Rule>, fallback: Name) -> Self {
        Self { rules, fallback }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn fallback(&self) -> &Name {
        &self.fallback
    }

    pub fn named(&self, domain: &Domain) -> Decision {
        match self.rules.iter().find(|rule| rule.named(domain)) {
            Some(rule) => Decision::new(rule.cluster().clone(), Ground::Named),
            None => Decision::new(self.fallback.clone(), Ground::Default),
        }
    }

    pub fn addressed(&self, address: IpAddr) -> Decision {
        match self.rules.iter().find(|rule| rule.addressed(address)) {
            Some(rule) => Decision::new(rule.cluster().clone(), Ground::Addressed),
            None => Decision::new(self.fallback.clone(), Ground::Default),
        }
    }
}
