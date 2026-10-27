use dynet_core::{Error, Instance, Range};
use std::collections::BTreeSet;

pub trait Projection: Send + Sync {
    fn settle(&self, wanted: &BTreeSet<Range>) -> Result<(), Error>;
}

impl Projection for Instance {
    fn settle(&self, wanted: &BTreeSet<Range>) -> Result<(), Error> {
        crate::host::Route::settle(self, wanted).map(|_| ())
    }
}
