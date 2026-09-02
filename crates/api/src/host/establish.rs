use super::{Cleared, Link, Route, Shape, Veil, reclaim, survey};
use dynet_core::{Error, Instance};

pub const BARE: &str = "nobody";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub cleared: Cleared,
    pub veiled: bool,
}

pub fn establish(
    instance: &Instance,
    port: u16,
    claim: &str,
    under: Option<&str>,
) -> Result<Standing, Error> {
    let shape = survey()?;
    if !shape.ownable() || matches!(shape, Shape::Resolved { upstream: false }) {
        return Err(Error::new(format!(
            "dynet will not start: {}",
            shape.explain()
        )));
    }
    let cleared = reclaim(instance)?;
    match raise(instance, port, claim, under) {
        Ok(()) => Ok(Standing {
            cleared,
            veiled: under != Some(BARE),
        }),
        Err(error) => Err(unwind(instance, error)),
    }
}

fn raise(instance: &Instance, port: u16, claim: &str, under: Option<&str>) -> Result<(), Error> {
    Link::create(instance)?;
    Route::create(instance, claim)?;
    if under == Some(BARE) {
        return Ok(());
    }
    Veil::raise(instance, port, under)
}

fn unwind(instance: &Instance, error: Error) -> Error {
    let unwound = match reclaim(instance) {
        Ok(cleared) => format!("cleared={}", cleared.any()),
        Err(second) => format!("unwind also failed: {second}"),
    };
    Error::new(format!(
        "{error}; the half-built state was unwound: {unwound}"
    ))
}
