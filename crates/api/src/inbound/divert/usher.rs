use super::{Divert, Held};
use dynet_core::Error;
use socket2::{Domain, Socket, Type};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::time::Duration;

const REST: Duration = Duration::from_millis(5);
const WAITING: i32 = 128;

pub(super) fn seat(port: u16) -> Result<TcpListener, Error> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)
        .map_err(|error| Error::new(format!("cannot open a transparent seat: {error}")))?;
    socket
        .set_ip_transparent(true)
        .map_err(|error| Error::new(format!("cannot make the seat transparent: {error}")))?;
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
    pub(super) fn usher(&self, listener: &TcpListener) {
        let _ = listener.set_nonblocking(true);
        std::thread::scope(|scope| {
            while !self.spent() {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(REST);
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                scope.spawn(|| self.admit(stream));
            }
        });
    }

    fn admit(&self, stream: TcpStream) {
        let Some(held) = handed(&stream) else {
            return;
        };
        (self.told())(&format!("{} ushered to {}", held.caller, held.target));
        self.tend(stream, held);
    }
}

fn handed(stream: &TcpStream) -> Option<Held> {
    let (SocketAddr::V4(caller), SocketAddr::V4(target)) =
        (stream.peer_addr().ok()?, stream.local_addr().ok()?)
    else {
        return None;
    };
    Some(shaped(caller, target))
}

fn shaped(caller: SocketAddrV4, target: SocketAddrV4) -> Held {
    Held { caller, target }
}
