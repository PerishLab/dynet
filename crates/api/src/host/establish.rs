use super::{Cleared, Link, Route, Shape, Veil, reclaim, survey};
use dynet_core::{Error, Instance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub cleared: Cleared,
    pub veiled: bool,
}

pub fn establish(instance: &Instance, port: u16) -> Result<Standing, Error> {
    let shape = survey()?;
    if !shape.ownable() || matches!(shape, Shape::Resolved { upstream: false }) {
        return Err(Error::new(format!(
            "dynet will not start: {}",
            shape.explain()
        )));
    }
    let cleared = reclaim(instance)?;
    match raise(instance, port) {
        Ok(()) => Ok(Standing {
            cleared,
            veiled: true,
        }),
        Err(error) => Err(unwind(instance, error)),
    }
}

fn raise(instance: &Instance, port: u16) -> Result<(), Error> {
    Link::create(instance)?;
    Route::create(instance)?;
    Veil::raise(instance, port)?;
    Ok(())
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
