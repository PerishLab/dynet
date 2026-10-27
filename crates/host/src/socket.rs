use socket2::Socket;

pub fn bind(socket: &Socket, mark: u32, six: bool) -> std::io::Result<()> {
    if mark == 0 {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    let _ = six;
    #[cfg(target_os = "linux")]
    let made = socket.set_mark(mark);
    #[cfg(windows)]
    let made = crate::windows::uplink().and_then(|index| {
        use std::os::windows::io::AsRawSocket;
        crate::windows::unicast(socket.as_raw_socket(), index, six)
    });
    #[cfg(target_os = "macos")]
    let made = crate::macos::uplink().and_then(|index| match six {
        true => socket.bind_device_by_index_v6(Some(index)),
        false => socket.bind_device_by_index_v4(Some(index)),
    });
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let made = {
        let _ = socket;
        Err::<(), _>(std::io::Error::from(std::io::ErrorKind::Unsupported))
    };
    made
}
