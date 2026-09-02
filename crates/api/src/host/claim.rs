use super::{DEVICE, attempt, run};
use dynet_core::Error;

pub struct Claim;

impl Claim {
    pub fn take(listener: &str) -> Result<(), Error> {
        run("resolvectl", &["dns", DEVICE, listener])?;
        run("resolvectl", &["domain", DEVICE, "~."])?;
        Ok(())
    }

    pub fn held() -> bool {
        run("resolvectl", &["domain", DEVICE]).is_ok_and(|text| text.contains("~."))
    }

    pub fn revert() -> bool {
        attempt("resolvectl", &["revert", DEVICE])
    }
}
