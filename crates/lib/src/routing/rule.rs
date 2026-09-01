use super::domain::Domain;
use super::range::Range;
use crate::cluster::Name;
use std::net::IpAddr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Subject {
    Exact(Domain),
    Suffix(Domain),
    Holds(Range),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    subject: Subject,
    cluster: Name,
}

impl Rule {
    pub fn new(subject: Subject, cluster: Name) -> Self {
        Self { subject, cluster }
    }

    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    pub fn cluster(&self) -> &Name {
        &self.cluster
    }

    pub fn named(&self, domain: &Domain) -> bool {
        match &self.subject {
            Subject::Exact(subject) => domain == subject,
            Subject::Suffix(subject) => domain.within(subject),
            Subject::Holds(_) => false,
        }
    }

    pub fn addressed(&self, address: IpAddr) -> bool {
        match &self.subject {
            Subject::Holds(range) => range.holds(address),
            Subject::Exact(_) | Subject::Suffix(_) => false,
        }
    }
}
