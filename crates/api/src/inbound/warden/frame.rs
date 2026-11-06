use std::io::{self, ErrorKind, Read, Write};
use std::time::{Duration, Instant};

pub(super) fn read(
    reader: &mut impl Read,
    live: impl Fn() -> bool,
    limits: (Duration, Duration),
) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0; 2];
    let mut deadline = Instant::now() + limits.0;
    if !fill(reader, &mut prefix, &live, (&mut deadline, Some(limits.1)))? {
        return Ok(None);
    }
    let size = usize::from(u16::from_be_bytes(prefix));
    if size < 12 {
        return Err(io::Error::new(ErrorKind::InvalidData, "short DNS frame"));
    }
    let mut body = vec![0; size];
    if !fill(reader, &mut body, &live, (&mut deadline, None))? {
        return Err(io::Error::new(ErrorKind::UnexpectedEof, "missing DNS body"));
    }
    Ok(Some(body))
}

fn fill(
    reader: &mut impl Read,
    room: &mut [u8],
    live: &impl Fn() -> bool,
    timing: (&mut Instant, Option<Duration>),
) -> io::Result<bool> {
    let (deadline, first) = timing;
    let mut offset = 0;
    while offset < room.len() {
        if !live() || Instant::now() >= *deadline {
            return Err(io::Error::new(ErrorKind::TimedOut, "DNS frame deadline"));
        }
        match reader.read(&mut room[offset..]) {
            Ok(0) if offset == 0 => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "partial DNS frame",
                ));
            }
            Ok(size) => {
                advance(deadline, first, offset);
                offset += size;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn advance(deadline: &mut Instant, first: Option<Duration>, offset: usize) {
    if let (0, Some(span)) = (offset, first) {
        *deadline = Instant::now() + span;
    }
}

pub(super) fn write(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let size = u16::try_from(body.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "large DNS frame"))?;
    let mut framed = Vec::with_capacity(body.len() + 2);
    framed.extend_from_slice(&size.to_be_bytes());
    framed.extend_from_slice(body);
    send(writer, &framed, Instant::now() + Duration::from_secs(2))
}

fn send(writer: &mut impl Write, mut body: &[u8], deadline: Instant) -> io::Result<()> {
    while !body.is_empty() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(ErrorKind::TimedOut, "DNS write deadline"));
        }
        let size = writer.write(body)?;
        if size == 0 {
            return Err(ErrorKind::WriteZero.into());
        }
        body = &body[size..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read, write};
    use std::io::{self, Cursor, ErrorKind, Read};
    use std::time::Duration;

    struct Fragments(Cursor<Vec<u8>>);

    impl Read for Fragments {
        fn read(&mut self, room: &mut [u8]) -> io::Result<usize> {
            let size = room.len().min(1);
            self.0.read(&mut room[..size])
        }
    }

    fn limits() -> (Duration, Duration) {
        (Duration::from_secs(1), Duration::from_secs(1))
    }

    #[test]
    fn pipeline() {
        let first = crate::resolver::ask("one.invalid", 0x1234).unwrap();
        let second = crate::resolver::ask("two.invalid", 0x4567).unwrap();
        let mut wire = Fragments(Cursor::new([first.clone(), second.clone()].concat()));
        assert_eq!(
            read(&mut wire, || true, limits()).unwrap().unwrap(),
            first[2..]
        );
        assert_eq!(
            read(&mut wire, || true, limits()).unwrap().unwrap(),
            second[2..]
        );
        assert!(read(&mut wire, || true, limits()).unwrap().is_none());
    }

    #[test]
    fn broken() {
        for bytes in [vec![0], vec![0, 11], vec![0, 12], vec![0, 12, 1]] {
            assert!(read(&mut Cursor::new(bytes), || true, limits()).is_err());
        }
    }

    #[test]
    fn bounds() {
        let mut wire = Vec::new();
        write(&mut wire, &vec![0; 65535]).unwrap();
        assert_eq!(
            read(&mut Cursor::new(wire), || true, limits())
                .unwrap()
                .unwrap()
                .len(),
            65535
        );
        let mut wire = Vec::new();
        assert!(write(&mut wire, &vec![0; 65536]).is_err());
        assert!(wire.is_empty());
    }

    #[test]
    fn cancellation() {
        let mut wire = Cursor::new(vec![0, 12, 0]);
        assert_eq!(
            read(&mut wire, || false, limits()).unwrap_err().kind(),
            ErrorKind::TimedOut
        );
        assert_eq!(wire.position(), 0);
        assert_eq!(
            read(&mut wire, || true, (Duration::ZERO, Duration::ZERO))
                .unwrap_err()
                .kind(),
            ErrorKind::TimedOut
        );
    }

    #[test]
    fn partial() {
        struct Stalled(bool);
        impl Read for Stalled {
            fn read(&mut self, room: &mut [u8]) -> io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    room[0] = 0;
                    return Ok(1);
                }
                std::thread::sleep(Duration::from_millis(1));
                Err(ErrorKind::WouldBlock.into())
            }
        }
        let mut wire = Stalled(false);
        assert_eq!(
            read(
                &mut wire,
                || true,
                (Duration::from_secs(1), Duration::from_millis(10))
            )
            .unwrap_err()
            .kind(),
            ErrorKind::TimedOut
        );
    }

    #[test]
    fn idle() {
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        let halt = AtomicBool::new(false);
        let started = std::time::Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                halt.store(true, Ordering::Release);
            });
            assert_eq!(
                read(
                    &mut stream,
                    || !halt.load(Ordering::Acquire),
                    (Duration::from_secs(10), Duration::from_secs(2))
                )
                .unwrap_err()
                .kind(),
                ErrorKind::TimedOut
            );
        });
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn writing() {
        struct Slow;
        impl std::io::Write for Slow {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                std::thread::sleep(Duration::from_millis(1));
                Ok(1)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(10);
        assert_eq!(
            super::send(&mut Slow, &[0; 100], deadline)
                .unwrap_err()
                .kind(),
            ErrorKind::TimedOut
        );
    }
}
