use super::{Cleared, Link, Route, Shape, Veil, reclaim, survey};
use dynet_core::{Error, Instance, Span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub cleared: Cleared,
    pub veiled: bool,
}

pub struct Ground<'a> {
    pub port: u16,
    pub claim: &'a str,
    pub span: Span,
    pub bare: bool,
    pub sources: &'a [String],
}

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
    Route::create(instance, ground.span, (ground.claim, ground.sources))?;
    if ground.bare {
        return Ok(());
    }
    Veil::raise(instance, ground.port)
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
