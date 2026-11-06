use super::{Call, REST, frame};
use crate::inbound::warren::Warren;
use crate::resolver::Packet;
use dynet_core::Error;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

pub(super) enum Reply<'a> {
    Datagram(&'a UdpSocket),
    Stream(&'a TcpStream),
}

impl Reply<'_> {
    pub fn send(&self, body: &[u8], peer: SocketAddr) -> io::Result<()> {
        match self {
            Self::Datagram(socket) => socket.send_to(body, peer).map(|_| ()),
            Self::Stream(socket) => {
                let mut socket = *socket;
                frame::write(&mut socket, body)
            }
        }
    }

    pub fn relay(&self, warren: &Warren, call: &Call) -> Result<Vec<u8>, Error> {
        let upstream = warren.current().upstream.clone();
        let asking = Packet::new(&call.asked);
        if matches!(self, Self::Datagram(_)) {
            return asking.relay(&upstream, warren.instance.mark());
        }
        let address: Ipv4Addr = upstream.parse().map_err(|_| refused("upstream address"))?;
        let socket = crate::outbound::reach(
            (address, 53).into(),
            warren.instance.mark(),
            Duration::from_secs(2),
        )?;
        let mut socket = call.circuit.attach(socket)?;
        socket
            .set_read_timeout(Some(REST))
            .map_err(|_| refused("read timeout"))?;
        socket
            .set_write_timeout(Some(REST))
            .map_err(|_| refused("write timeout"))?;
        frame::write(&mut socket, &call.asked).map_err(|_| refused("query write"))?;
        let live = || !warren.halted() && !call.circuit.closed();
        let body = frame::read(
            &mut socket,
            live,
            (Duration::from_secs(8), Duration::from_secs(2)),
        )
        .map_err(|_| refused("answer read"))?
        .ok_or_else(|| refused("empty answer"))?;
        if body[..2] != call.asked[..2] {
            return Err(refused("answer identity"));
        }
        Ok(body)
    }
}

fn refused(part: &str) -> Error {
    Error::new(format!("TCP DNS {part} failed"))
}
