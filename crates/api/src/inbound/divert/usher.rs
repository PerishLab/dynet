use super::{Divert, Held};
use dynet_core::Error;
#[cfg(target_os = "linux")]
use socket2::SockRef;
use socket2::{Domain, Socket, Type};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::time::Duration;

const REST: Duration = Duration::from_millis(5);
const WAITING: i32 = 128;

pub(super) fn seat(port: u16) -> Result<TcpListener, Error> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)
        .map_err(|error| Error::new(format!("cannot open a transparent seat: {error}")))?;
    #[cfg(target_os = "linux")]
    let made = socket.set_ip_transparent(true);
    #[cfg(not(target_os = "linux"))]
    let made = Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
    made.map_err(|error| Error::new(format!("cannot make the seat transparent: {error}")))?;
    socket
        .set_reuse_address(true)
        .map_err(|error| Error::new(format!("cannot hold the transparent seat: {error}")))?;
    let held = SocketAddr::from(([0, 0, 0, 0], port));
    socket
        .bind(&held.into())
        .map_err(|error| Error::new(format!("cannot bind the transparent seat: {error}")))?;
    socket
        .listen(WAITING)
        .map_err(|error| Error::new(format!("cannot seat the usher: {error}")))?;
    Ok(socket.into())
}

impl Divert<'_> {
    pub(super) fn usher(&self, listener: &TcpListener, transit: u16) {
        let _ = listener.set_nonblocking(true);
        std::thread::scope(|scope| {
            while !self.spent() {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(REST);
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                scope.spawn(|| self.admit(stream, transit));
            }
        });
    }

    fn admit(&self, stream: TcpStream, transit: u16) {
        let Some(held) = handed(&stream, transit) else {
            return;
        };
        (self.told())(&format!("{} ushered to {}", held.caller, held.target));
        self.tend(stream, held);
    }
}

fn handed(stream: &TcpStream, transit: u16) -> Option<Held> {
    let SocketAddr::V4(caller) = stream.peer_addr().ok()? else {
        return None;
    };
    let SocketAddr::V4(target) = borne(stream, transit)? else {
        return None;
    };
    Some(shaped(caller, target))
}

fn borne(stream: &TcpStream, transit: u16) -> Option<SocketAddr> {
    let seated = stream.local_addr().ok()?;
    if seated.port() != transit {
        return Some(seated);
    }
    #[cfg(target_os = "linux")]
    return SockRef::from(stream).original_dst().ok()?.as_socket();
    #[cfg(not(target_os = "linux"))]
    None
}

fn shaped(caller: SocketAddrV4, target: SocketAddrV4) -> Held {
    Held { caller, target }
}
