use super::{Link, Route, Shape, Veil, reclaim, survey};
use crate::host::{Ground, Standing};
use dynet_core::{Error, Instance};

pub fn establish(instance: &Instance, ground: &Ground) -> Result<Standing, Error> {
    let shape = survey()?;
    if !shape.ownable() || matches!(shape, Shape::Resolved { upstream: false }) {
        return Err(Error::new(format!(
            "dynet will not start: {}",
            shape.explain()
        )));
    }
    let cleared = reclaim(instance)?;
    match raise(instance, ground) {
        Ok(()) => Ok(Standing {
            cleared,
            veiled: !ground.bare,
        }),
        Err(error) => Err(unwind(instance, error)),
    }
}

fn raise(instance: &Instance, ground: &Ground) -> Result<(), Error> {
    Link::create(instance, ground.span)?;
    Route::create(
        instance,
        ground.span,
        (ground.claim, ground.sources, ground.transit),
    )?;
    if ground.bare {
        return Ok(());
    }
    Veil::raise(
        instance,
        (ground.span, ground.port, ground.transit),
        ground.sources,
    )
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
