use dynet_api::{
    Loaded,
    control::{Private, Session},
    host::State,
};
use dynet_core::{Error, Instance};
use std::path::Path;

pub fn read(path: &Path, instance: &Instance, state: &State) -> Result<Session, Error> {
    Session::open(path, instance, state, Private::current())
}

pub fn loaded(session: &Session) -> Result<&Loaded, Error> {
    session
        .initial
        .payload
        .as_ref()
        .map(|payload| &payload.loaded)
        .ok_or_else(|| Error::new("the signed configuration is revoked"))
}
