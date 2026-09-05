use dynet_core::Error;
use socket2::{Domain, Protocol, Socket, TcpKeepalive, Type};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

const PULSE: Duration = Duration::from_secs(10);
const TRIES: u32 = 3;

pub fn reach(seat: SocketAddr, mark: u32, patience: Duration) -> Result<TcpStream, Error> {
    let domain = Domain::for_address(seat);
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .map_err(|error| Error::new(format!("cannot open a node socket: {error}")))?;
    stamp(&socket, mark)?;
    rouse(&socket)?;
    socket
        .connect_timeout(&seat.into(), patience)
        .map_err(|error| Error::new(format!("cannot reach the node: {error}")))?;
    Ok(socket.into())
}

pub fn marked(mark: u32) -> Result<UdpSocket, Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| Error::new(format!("cannot open a resolving socket: {error}")))?;
    stamp(&socket, mark)?;
    socket
        .bind(&SocketAddr::from(([0, 0, 0, 0], 0)).into())
        .map_err(|error| Error::new(format!("cannot seat a resolving socket: {error}")))?;
    Ok(socket.into())
}

fn rouse(socket: &Socket) -> Result<(), Error> {
    let alive = TcpKeepalive::new()
        .with_time(PULSE)
        .with_interval(PULSE)
        .with_retries(TRIES);
    socket.set_tcp_keepalive(&alive).map_err(|error| {
        Error::new(format!(
            "cannot ask a node session to answer for itself: {error}"
        ))
    })
}

fn stamp(socket: &Socket, mark: u32) -> Result<(), Error> {
    if mark == 0 {
        return Ok(());
    }
    socket
        .set_mark(mark)
        .map_err(|error| Error::new(format!("cannot mark a socket as Dynet's own: {error}")))
}
