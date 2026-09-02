use crate::Cluster;
use crate::error::Error;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Spread {
    Wide,
    Bound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Affinity {
    window: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    cluster: Cluster,
    affinity: Option<Affinity>,
}

impl Affinity {
    pub fn new(window: Duration) -> Result<Self, Error> {
        if window.is_zero() {
            return Err(Error::new("an affinity window must outlast an instant"));
        }
        Ok(Self { window })
    }

    pub fn window(self) -> Duration {
        self.window
    }
}

impl Pool {
    pub fn wide(cluster: Cluster) -> Self {
        Self {
            cluster,
            affinity: None,
        }
    }

    pub fn bound(cluster: Cluster, affinity: Affinity) -> Self {
        Self {
            cluster,
            affinity: Some(affinity),
        }
    }

    pub fn cluster(&self) -> &Cluster {
        &self.cluster
    }

    pub fn affinity(&self) -> Option<Affinity> {
        self.affinity
    }

    pub fn spread(&self) -> Spread {
        match self.affinity {
            None => Spread::Wide,
            Some(_) => Spread::Bound,
        }
    }

    pub fn datagrams(&self) -> bool {
        self.cluster.datagrams()
    }
}
