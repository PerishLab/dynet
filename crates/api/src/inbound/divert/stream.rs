use super::Divert;
use crate::inbound::IDLE;
use crate::inbound::link::{Link, Taken};
use crate::inbound::warren::Charge;
use crate::outbound::Circuit;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

const REST: Duration = Duration::from_millis(200);

// An open, idle stream remains valid. Only a half-closed stream has a quiet
// deadline, and continued traffic in either direction extends that deadline.
struct Progress {
    half_closed: Mutex<Option<Instant>>,
    quiet: Duration,
}

impl Progress {
    fn touch(&self) {
        if let Ok(mut held) = self.half_closed.lock()
            && held.is_some()
        {
            *held = Some(Instant::now());
        }
    }

    fn finish(&self) {
        if let Ok(mut held) = self.half_closed.lock() {
            held.get_or_insert_with(Instant::now);
        }
    }

    fn expired(&self) -> bool {
        self.half_closed.lock().map_or(true, |held| {
            held.is_some_and(|when| when.elapsed() >= self.quiet)
        })
    }
}

impl Divert<'_> {
    pub(in crate::inbound) fn carry(
        &self,
        near: TcpStream,
        mut link: Link,
        charge: Option<&Charge>,
    ) {
        let _ = relay(near, &mut link, || self.spent(), IDLE);
        self.judge(&link, charge);
    }
}

fn relay(
    near: TcpStream,
    link: &mut Link,
    stopped: impl Fn() -> bool + Sync,
    quiet: Duration,
) -> std::io::Result<()> {
    let circuit = link.circuit();
    let mut near = match &circuit {
        Some(circuit) => circuit.attach(near).map_err(std::io::Error::other)?,
        None => near,
    };
    let again = near.try_clone()?;
    again.set_read_timeout(Some(REST.min(quiet)))?;
    near.set_write_timeout(Some(IDLE))?;
    let progress = Progress {
        half_closed: Mutex::new(None),
        quiet,
    };
    let sending = link.upward();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            push(again, sending, circuit.as_deref(), &progress, &stopped);
            progress.finish();
        });
        while !stopped() && !progress.expired() {
            match link.bide(REST.min(quiet)) {
                Taken::Body(body) => {
                    if near.write_all(&body).is_err() {
                        let _ = near.shutdown(Shutdown::Both);
                        break;
                    }
                    progress.touch();
                }
                Taken::Spent => break,
                Taken::Empty => {}
            }
        }
        progress.finish();
        let _ = near.shutdown(Shutdown::Write);
        if stopped() || progress.expired() || link.closed() {
            let _ = near.shutdown(Shutdown::Both);
        }
    });
    let _ = near.shutdown(Shutdown::Both);
    Ok(())
}

fn push(
    mut from: TcpStream,
    sending: Option<SyncSender<Vec<u8>>>,
    circuit: Option<&Circuit>,
    progress: &Progress,
    stopped: &impl Fn() -> bool,
) {
    let Some(sending) = sending else { return };
    let closed = || stopped() || progress.expired() || circuit.is_some_and(Circuit::closed);
    let mut room = [0u8; 16384];
    while !closed() {
        let size = match from.read(&mut room) {
            Ok(0) => break,
            Ok(size) => size,
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                continue;
            }
            Err(_) => break,
        };
        progress.touch();
        let mut body = room[..size].to_vec();
        while !closed() {
            match sending.try_send(body) {
                Ok(()) => break,
                Err(TrySendError::Full(held)) => body = held,
                Err(TrySendError::Disconnected(_)) => return,
            }
            std::thread::sleep(REST);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::relay;
    use crate::inbound::Link;
    use crate::outbound::Passage;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::time::Duration;

    const BOUND: Duration = Duration::from_secs(3);
    const QUIET: Duration = Duration::from_millis(100);

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        client.set_read_timeout(Some(BOUND)).unwrap();
        server.set_read_timeout(Some(BOUND)).unwrap();
        (client, server)
    }

    #[test]
    fn downstream_eof_releases_a_client_that_keeps_its_write_half_open() {
        let remote = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let (mut client, near) = pair();
        let (done, ended) = mpsc::channel();
        let port = remote.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut link = Link::open(Passage::Direct(0), "127.0.0.1".into(), port);
            relay(near, &mut link, || false, QUIET).unwrap();
            drop(link);
            done.send(()).unwrap();
        });
        let (far, _) = remote.accept().unwrap();
        far.shutdown(Shutdown::Write).unwrap();
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
        ended.recv_timeout(BOUND).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn upstream_half_close_still_delivers_the_response() {
        let remote = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = remote.local_addr().unwrap().port();
        let (mut client, near) = pair();
        let worker = std::thread::spawn(move || {
            let mut link = Link::open(Passage::Direct(0), "127.0.0.1".into(), port);
            relay(near, &mut link, || false, QUIET).unwrap();
        });
        let (mut far, _) = remote.accept().unwrap();
        far.set_read_timeout(Some(BOUND)).unwrap();
        client.write_all(b"request").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut request = Vec::new();
        far.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"request");
        far.write_all(b"response").unwrap();
        far.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"response");
        worker.join().unwrap();
    }

    #[test]
    fn idle_open_stream_is_not_subject_to_the_half_close_deadline() {
        let remote = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = remote.local_addr().unwrap().port();
        let (mut client, near) = pair();
        let worker = std::thread::spawn(move || {
            let mut link = Link::open(Passage::Direct(0), "127.0.0.1".into(), port);
            relay(near, &mut link, || false, QUIET).unwrap();
        });
        let (mut far, _) = remote.accept().unwrap();
        std::thread::sleep(QUIET * 3);
        far.write_all(b"still open").unwrap();
        let mut body = [0; 10];
        client.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"still open");
        client.shutdown(Shutdown::Both).unwrap();
        far.shutdown(Shutdown::Both).unwrap();
        worker.join().unwrap();
    }
}
