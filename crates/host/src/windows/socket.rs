use std::io::{Error, Result};
use std::os::windows::io::RawSocket;
use windows_sys::Win32::Networking::WinSock::{
    IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, SOCKET, setsockopt,
};

pub fn unicast(socket: RawSocket, index: u32, six: bool) -> Result<()> {
    let (level, option, value) = match six {
        true => (IPPROTO_IPV6, IPV6_UNICAST_IF, index),
        false => (IPPROTO_IP, IP_UNICAST_IF, index.to_be()),
    };
    let made = unsafe {
        setsockopt(
            socket as SOCKET,
            level,
            option,
            (&value as *const u32).cast(),
            size_of::<u32>() as i32,
        )
    };
    match made {
        0 => Ok(()),
        _ => Err(Error::last_os_error()),
    }
}
