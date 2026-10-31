use super::{Link, Route, Shape, Veil, reclaim, survey};
use crate::host::{Ground, Standing};
use dynet_core::{Error, Instance};

pub fn establish(instance: &Instance, ground: &Ground) -> Result<Standing, Error> {
    if !ground.local && ground.sources.is_empty() {
        return Err(Error::new(
            "forwarded-only capture requires declared sources",
        ));
    }
    if ground.local {
        resolver()?;
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

fn resolver() -> Result<(), Error> {
    let shape = survey()?;
    if !shape.ownable() || matches!(shape, Shape::Resolved { upstream: false }) {
        return Err(Error::new(format!(
            "dynet will not start: {}",
            shape.explain()
        )));
    }
    Ok(())
}

fn raise(instance: &Instance, ground: &Ground) -> Result<(), Error> {
    Link::create(instance, ground.span)?;
    Route::create(
        instance,
        ground.span,
        (ground.claim, ground.sources, ground.transit),
        ground.local,
    )?;
    if ground.bare {
        return Ok(());
    }
    Veil::raise(
        instance,
        (ground.span, ground.port, ground.transit),
        ground.sources,
        ground.local,
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
