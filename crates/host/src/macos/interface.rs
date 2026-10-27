use std::ffi::CString;
use std::io::{Error, Result};

pub fn name(instance: &str) -> String {
    let hash = instance.bytes().fold(2166136261u32, |held, byte| {
        (held ^ u32::from(byte)).wrapping_mul(16777619)
    });
    format!("utun{}", 100 + hash % 30000)
}

pub fn index(name: &str) -> Result<u32> {
    let name = CString::new(name).map_err(|_| Error::other("an interface name contains zero"))?;
    match unsafe { libc::if_nametoindex(name.as_ptr()) } {
        0 => Err(Error::last_os_error()),
        index => Ok(index),
    }
}
