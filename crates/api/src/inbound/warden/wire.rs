use crate::host::Route;
use crate::inbound::warren::Chosen;
use crate::outbound::Listener;
use dynet_core::{Error, Fault};
use std::net::IpAddr;

pub(super) fn through(chosen: &Chosen, upstream: &str, framed: &[u8]) -> Result<Vec<u8>, Fault> {
    let (mut speaker, mut listener) = chosen.passage.open(upstream, 53)?;
    speaker.send(framed).map_err(|_| Fault::Severed)?;
    gather(&mut listener).map_err(|_| listener.fault().unwrap_or(Fault::Silent))
}

pub(super) fn capture(instance: &dynet_core::Instance, address: IpAddr) -> Result<(), Error> {
    match Route::hold(instance, address) {
        true => Ok(()),
        false => Err(Error::new(format!(
            "cannot capture {address}, so its answer is withheld"
        ))),
    }
}

fn gather(listener: &mut Listener) -> Result<Vec<u8>, Error> {
    let mut framed = Vec::new();
    while let Some(part) = listener.receive()? {
        framed.extend_from_slice(&part);
        if framed.len() >= 2 {
            let want = usize::from(u16::from_be_bytes([framed[0], framed[1]]));
            if framed.len() >= want + 2 {
                break;
            }
        }
    }
    if framed.len() < 2 {
        return Err(Error::new("the upstream answered nothing"));
    }
    Ok(framed)
}
