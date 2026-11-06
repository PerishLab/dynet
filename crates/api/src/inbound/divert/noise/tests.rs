use super::frame::{Frame, LIMIT, WINDOW};
use super::{Key, Noise};
use dynet_core::Name;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[test]
fn window() {
    let now = Instant::now();
    let mut frame = Frame::new(now);
    let key = Key::Multicast(5353);
    for index in 0..1000 {
        assert_eq!(frame.record(&key, now), (index == 0, false));
    }
    assert!(
        frame
            .rotate(now + WINDOW - Duration::from_millis(1), false)
            .is_empty()
    );
    let lines = frame.rotate(now + WINDOW, false);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("window=0 scope=bucket class=multicast port=5353"));
    assert!(lines[0].contains("seen=1000 first=1 suppressed=999"));
    assert!(lines[0].contains("first_ms=0 last_ms=0 window_ms=60000"));
    assert_eq!(frame.record(&key, now + WINDOW), (true, false));
    assert!(frame.rotate(now + WINDOW, true).is_empty());
    assert!(frame.rotate(now + WINDOW + WINDOW, false).is_empty());
}

#[test]
fn overflow() {
    let now = Instant::now();
    let mut frame = Frame::new(now);
    for port in 0..LIMIT {
        assert_eq!(
            frame.record(&Key::Multicast(port as u16), now),
            (true, false)
        );
    }
    for port in 1000..2000 {
        assert_eq!(
            frame.record(&Key::Multicast(port), now),
            (port == 1000, true)
        );
    }
    let key = Key::Unborne(Name::new("L1").unwrap(), 27036);
    for index in 0..100 {
        assert_eq!(frame.record(&key, now), (index == 0, true));
    }
    assert_eq!(frame.record(&Key::Multicast(1), now), (false, false));
    let lines = frame.rotate(now + WINDOW, false);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().any(|line| {
        line.contains("scope=overflow class=multicast seen=1000 first=1 suppressed=999")
    }));
    assert!(
        lines.iter().any(
            |line| line.contains("scope=overflow class=unborne seen=100 first=1 suppressed=99")
        )
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("scope=bucket class=multicast port=1 seen=2"))
    );
    assert_eq!(frame.record(&key, now + WINDOW), (true, false));
}

fn repeat(noise: &Noise, told: &(dyn Fn(&str) + Sync)) {
    for _ in 0..1000 {
        noise.refuse(Key::Multicast(5353), || "refused".into(), told);
    }
}

#[test]
fn concurrent() {
    let noise = Noise::new();
    let lines = Mutex::new(Vec::new());
    let told = |line: &str| lines.lock().unwrap().push(line.to_string());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| repeat(&noise, &told));
        }
    });
    noise.flush(true, &told);
    let lines = lines.into_inner().unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("udp refusal first window=0 scope=bucket"));
    assert!(lines[1].contains("seen=8000 first=1 suppressed=7999"));
}

#[test]
fn sink() {
    let noise = Noise::new();
    let called = std::sync::atomic::AtomicUsize::new(0);
    let told = |_: &str| {
        assert!(noise.frame.try_lock().is_ok());
        called.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    };
    noise.refuse(Key::Multicast(5353), || "first".into(), &told);
    noise.refuse(
        Key::Multicast(5353),
        || panic!("suppressed formatting ran"),
        &told,
    );
    noise.flush(true, &told);
    assert_eq!(called.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[test]
fn poisoned() {
    let noise = Noise::new();
    std::thread::scope(|scope| {
        assert!(
            scope
                .spawn(|| {
                    let _guard = noise.frame.lock().unwrap();
                    panic!("owned counter failure");
                })
                .join()
                .is_err()
        );
    });
    let lines = Mutex::new(Vec::new());
    let told = |line: &str| {
        assert!(!matches!(
            noise.frame.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        noise.flush(true, &|_| panic!("duplicate failure"));
        lines.lock().unwrap().push(line.to_string());
    };
    noise.refuse(
        Key::Multicast(5353),
        || panic!("unreliable accounting"),
        &told,
    );
    noise.refuse(
        Key::Multicast(1900),
        || panic!("unreliable accounting"),
        &told,
    );
    noise.flush(true, &told);
    let lines = lines.into_inner().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("counts are unknown, packet handling is unchanged"));
}
