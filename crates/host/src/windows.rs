mod filter;
mod folder;
mod route;
mod service;
mod socket;
mod uplink;

pub use filter::{Settled, Wall};
pub use folder::data;
pub use route::{Interface, Prefix};
pub use service::{closing, halted, ready, serve};
pub use socket::unicast;
pub use uplink::uplink;

fn settle(code: u32) -> std::io::Result<()> {
    match code {
        0 => Ok(()),
        code => Err(std::io::Error::from_raw_os_error(code as i32)),
    }
}

fn widen(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

mod device;
pub(crate) use device::Device;
