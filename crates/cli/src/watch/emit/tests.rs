use super::*;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

struct Sink {
    bytes: Arc<Mutex<Vec<u8>>>,
    blocked: Option<(Sender<()>, Receiver<()>)>,
    failed: Arc<AtomicBool>,
}

impl Write for Sink {
    fn write(&mut self, body: &[u8]) -> io::Result<usize> {
        if let Some((ready, release)) = self.blocked.take() {
            ready.send(()).unwrap();
            release.recv().unwrap();
        }
        if self.failed.load(Ordering::Relaxed) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.bytes.lock().unwrap().extend_from_slice(body);
        Ok(body.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn seat() -> tempfile::TempDir {
    let seat = tempfile::tempdir().unwrap();
    fs::set_permissions(seat.path(), fs::Permissions::from_mode(0o700)).unwrap();
    seat
}

fn until(root: &Path, value: u64) {
    let path = root.join("detail.until");
    fs::write(&path, value.to_string()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn wait(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn text(bytes: &Mutex<Vec<u8>>) -> String {
    String::from_utf8(bytes.lock().unwrap().clone()).unwrap()
}

fn sink() -> Sink {
    Sink {
        bytes: Arc::new(Mutex::new(Vec::new())),
        blocked: None,
        failed: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn blocked() {
    let seat = seat();
    let mut writer = sink();
    let bytes = Arc::clone(&writer.bytes);
    let (ready, entered) = channel();
    let (release, blocked) = channel();
    writer.blocked = Some((ready, blocked));
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", "refused: first");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let started = Instant::now();
    for _ in 0..1000 {
        emit.tell("dynettest", "refused: saturated");
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(emit.queue.as_ref().unwrap().lost.load(Ordering::Relaxed) > 0);
    release.send(()).unwrap();
    wait(|| text(&bytes).contains("lost="));
    emit.tell("dynettest", "refused: recovered");
    wait(|| text(&bytes).contains("refused: recovered"));
}

#[test]
fn failed() {
    let seat = seat();
    let writer = sink();
    let bytes = Arc::clone(&writer.bytes);
    let failed = Arc::clone(&writer.failed);
    failed.store(true, Ordering::Relaxed);
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", "refused: unavailable output");
    wait(|| emit.queue.as_ref().unwrap().lost.load(Ordering::Relaxed) > 0);
    failed.store(false, Ordering::Relaxed);
    emit.tell("dynettest", "refused: output recovered");
    wait(|| text(&bytes).contains("refused: output recovered"));
    assert!(text(&bytes).contains("lost=1"));
}

#[test]
fn expiry() {
    let seat = seat();
    let clock = Clock::open().unwrap();
    until(seat.path(), clock.now() + 1);
    let writer = sink();
    let bytes = Arc::clone(&writer.bytes);
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", "example.test left to the upstream");
    wait(|| text(&bytes).contains("example.test"));
    std::thread::sleep(Duration::from_millis(1200));
    emit.tell("dynettest", "expired.test left to the upstream");
    emit.tell("dynettest", "refused: after expiry");
    wait(|| text(&bytes).contains("after expiry"));
    assert!(!text(&bytes).contains("expired.test"));
    until(seat.path(), clock.now() + 301);
    assert_eq!(clock.deadline(seat.path()), 0);
}

#[test]
fn control() {
    let seat = seat();
    let mut writer = sink();
    let (ready, entered) = channel();
    let (release, blocked) = channel();
    writer.blocked = Some((ready, blocked));
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", "refused: blocked output");
    entered.recv_timeout(Duration::from_secs(3)).unwrap();
    let queue = emit.queue.as_ref().unwrap();
    until(seat.path(), queue.clock.now() + 30);
    wait(|| queue.until.load(Ordering::Relaxed) > queue.clock.now());
    until(seat.path(), 0);
    wait(|| queue.until.load(Ordering::Relaxed) == 0);
    release.send(()).unwrap();
}

#[test]
fn custody() {
    let seat = seat();
    Clock::private(seat.path(), true).unwrap();
    fs::set_permissions(seat.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Clock::private(seat.path(), true).is_err());
    fs::set_permissions(seat.path(), fs::Permissions::from_mode(0o700)).unwrap();
    until(seat.path(), 1);
    let link = seat.path().join("linked");
    fs::hard_link(seat.path().join("detail.until"), &link).unwrap();
    assert!(Clock::private(&link, false).is_err());
}

#[test]
fn conservative() {
    assert_eq!(
        classification("example.test left to the upstream"),
        "routine"
    );
    assert_eq!(
        classification("example.test through a gave 203.0.113.1"),
        "routine"
    );
    for line in [
        "refused: the upstream gave an invalid answer",
        "configuration application refused; confirmed state retained",
        "UDP refusal statistics unavailable",
        "previously unknown diagnostic text",
    ] {
        assert_ne!(classification(line), "routine");
    }
}

#[test]
fn bounded() {
    let seat = seat();
    let writer = sink();
    let bytes = Arc::clone(&writer.bytes);
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", &format!("refused: {}", "界".repeat(2000)));
    wait(|| text(&bytes).contains("truncated=true"));
    assert!(bytes.lock().unwrap().len() < 1024);
}

#[test]
fn escaped() {
    let seat = seat();
    let writer = sink();
    let bytes = Arc::clone(&writer.bytes);
    let emit = Emit::buffered(seat.path().to_path_buf(), "dynettest", writer).unwrap();
    emit.tell("dynettest", "refused: first\nforged\rline");
    wait(|| text(&bytes).contains("forged"));
    let body = text(&bytes);
    assert!(body.contains("first\\nforged\\rline"));
    assert_eq!(body.lines().count(), 2);
}
