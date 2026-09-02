use dynet_core::Fault;
use std::io::ErrorKind;

pub fn ending(kind: ErrorKind) -> Fault {
    match kind {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Fault::Silent,
        _ => Fault::Severed,
    }
}
