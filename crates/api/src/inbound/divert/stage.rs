use crate::host::Link;
use crate::inbound::warren::Warren;
use dynet_core::{Error, Span};
use dynet_host::Device;
use std::net::{TcpListener, UdpSocket};
use std::time::{Duration, Instant};

const PATIENCE: Duration = Duration::from_secs(10);
const REST: Duration = Duration::from_millis(100);

pub(super) struct Seats {
    pub device: Device,
    pub listener: TcpListener,
    pub darts: UdpSocket,
    pub resolver: UdpSocket,
}

pub(super) fn stage(warren: &Warren, span: Span) -> Result<Seats, Error> {
    let device = Device::open(warren.instance.get())
        .map_err(|error| Error::new(format!("cannot open the device: {error}")))?;
    Link::dress(warren.instance, span)?;
    warren.settle()?;
    let started = Instant::now();
    loop {
        let seated = TcpListener::bind((span.seat(), 0))
            .and_then(|listener| UdpSocket::bind((span.seat(), 0)).map(|darts| (listener, darts)));
        match seated {
            Ok((listener, darts)) => {
                let resolver = UdpSocket::bind((span.seat(), warren.port)).map_err(refused)?;
                warren.confirmed()?;
                return Ok(Seats {
                    device,
                    listener,
                    darts,
                    resolver,
                });
            }
            Err(error) if started.elapsed() >= PATIENCE => {
                return Err(Error::new(format!(
                    "cannot seat the diverter on {}: {error}",
                    span.seat()
                )));
            }
            Err(_) => std::thread::sleep(REST),
        }
    }
}

fn refused(error: std::io::Error) -> Error {
    Error::new(format!("cannot hold the resolver port: {error}"))
}
