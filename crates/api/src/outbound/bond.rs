use dynet_core::{Error, Fault};
use socket2::{Domain, Protocol, Socket, TcpKeepalive, Type};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

const PULSE: Duration = Duration::from_secs(10);
#[cfg(unix)]
const TRIES: u32 = 3;

pub fn reach(seat: SocketAddr, mark: u32, patience: Duration) -> Result<TcpStream, Error> {
    let domain = Domain::for_address(seat);
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .map_err(|error| Error::new(format!("cannot open a node socket: {error}")))?;
    stamp(&socket, mark, domain == Domain::IPV6)?;
    rouse(&socket)?;
    socket
        .connect_timeout(&seat.into(), patience)
        .map_err(|error| Error::new(format!("cannot reach the node: {error}")))?;
    Ok(socket.into())
}

pub fn marked(mark: u32) -> Result<UdpSocket, Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| Error::new(format!("cannot open a resolving socket: {error}")))?;
    stamp(&socket, mark, false)?;
    socket
        .bind(&SocketAddr::from(([0, 0, 0, 0], 0)).into())
        .map_err(|error| Error::new(format!("cannot seat a resolving socket: {error}")))?;
    Ok(socket.into())
}

fn rouse(socket: &Socket) -> Result<(), Error> {
    let alive = TcpKeepalive::new().with_time(PULSE).with_interval(PULSE);
    #[cfg(unix)]
    let alive = alive.with_retries(TRIES);
    socket.set_tcp_keepalive(&alive).map_err(|error| {
        Error::new(format!(
            "cannot ask a node session to answer for itself: {error}"
        ))
    })
}

fn stamp(socket: &Socket, mark: u32, six: bool) -> Result<(), Error> {
    if mark == 0 {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    let _ = six;
    #[cfg(target_os = "linux")]
    let made = socket.set_mark(mark);
    #[cfg(windows)]
    let made = dynet_windows::uplink().and_then(|index| {
        use std::os::windows::io::AsRawSocket;
        dynet_windows::unicast(socket.as_raw_socket(), index, six)
    });
    #[cfg(target_os = "macos")]
    let made = crate::host::macos::uplink().and_then(|index| match six {
        true => socket.bind_device_by_index_v6(Some(index)),
        false => socket.bind_device_by_index_v4(Some(index)),
    });
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let made = {
        let _ = socket;
        Err::<(), _>(std::io::Error::from(ErrorKind::Unsupported))
    };
    made.map_err(|error| Error::new(format!("cannot mark a socket as Dynet's own: {error}")))
}

pub fn ending(kind: ErrorKind) -> Fault {
    match kind {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Fault::Silent,
        _ => Fault::Severed,
    }
}

pub(super) fn place(host: &str, port: u16) -> Result<SocketAddr, Error> {
    let named = format!("{host}:{port}");
    if let Ok(seat) = named.parse() {
        return Ok(seat);
    }
    named
        .to_socket_addrs()
        .map_err(|error| Error::new(format!("cannot place the node: {error}")))?
        .next()
        .ok_or_else(|| Error::new(format!("{host} names no address")))
}
