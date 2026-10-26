#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod filter;
#[cfg(windows)]
mod route;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod socket;
#[cfg(windows)]
mod uplink;

#[cfg(windows)]
pub use filter::{Settled, Wall};
#[cfg(windows)]
pub use route::{Interface, Prefix};
#[cfg(windows)]
pub use service::{closing, halted, ready, serve};
#[cfg(windows)]
pub use socket::unicast;
#[cfg(windows)]
pub use uplink::uplink;

#[cfg(windows)]
fn settle(code: u32) -> std::io::Result<()> {
    match code {
        0 => Ok(()),
        code => Err(std::io::Error::from_raw_os_error(code as i32)),
    }
}

#[cfg(windows)]
fn widen(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
