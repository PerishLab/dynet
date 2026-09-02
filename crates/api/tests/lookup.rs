use dynet_api::outbound::Book;
use dynet_api::resolver;
use std::env;
use std::time::{Duration, Instant};

#[test]
fn literal() {
    let book = Book::new(0);
    let held = book
        .find("203.0.113.9", "203.0.113.1")
        .expect("an address needs no lookup");
    assert_eq!(held, "203.0.113.9", "an address is returned as it stands");
}

fn upstream() -> Option<String> {
    env::var("DYNET_UPSTREAM").ok()
}

#[test]
#[ignore]
fn carries() {
    let held = upstream().expect("set DYNET_UPSTREAM to a reachable resolver");
    let name = env::var("DYNET_NAME").expect("set DYNET_NAME to a node hostname");
    let answer = resolver::locate(&name, &held, 0).expect("a lookup");
    println!("{name} {} life {}", answer.address, answer.life);
    assert!(
        answer.life > 0,
        "a vendor name must carry a time to live for the store to honour"
    );
}

#[test]
#[ignore]
fn hurries() {
    let held = upstream().expect("set DYNET_UPSTREAM to a reachable resolver");
    let name = env::var("DYNET_NAME").expect("set DYNET_NAME to a node hostname");
    let book = Book::new(0);
    let cold = Instant::now();
    let first = book.find(&name, &held).expect("a first lookup");
    let asked = cold.elapsed();
    let warm = Instant::now();
    let second = book.find(&name, &held).expect("a remembered lookup");
    let recalled = warm.elapsed();
    println!("asked {asked:?} recalled {recalled:?}");
    assert_eq!(first, second, "a hit inside the life returns what was kept");
    assert!(
        recalled < Duration::from_millis(1),
        "a hit must not wait on the network: {recalled:?}"
    );
    assert!(asked > recalled, "the first lookup is the one that pays");
}

#[test]
#[ignore]
fn follows() {
    let held = upstream().expect("set DYNET_UPSTREAM to a reachable resolver");
    let name = env::var("DYNET_NAME").expect("set DYNET_NAME to a node hostname");
    let book = Book::new(0);
    let answer = resolver::locate(&name, &held, 0).expect("a lookup");
    let life = Duration::from_secs(u64::from(answer.life)).max(Duration::from_secs(1));
    book.find(&name, &held).expect("a first lookup");
    let mut seen = Vec::new();
    for _ in 0..8 {
        std::thread::sleep(life);
        let started = Instant::now();
        seen.push(book.find(&name, &held).expect("a later lookup"));
        assert!(
            started.elapsed() < Duration::from_millis(1),
            "a lookup past the life must still be served from memory while it refreshes"
        );
    }
    println!("addresses seen {seen:?}");
    assert!(
        seen.iter()
            .all(|held| held.parse::<std::net::Ipv4Addr>().is_ok()),
        "every answer stays a usable address across refreshes"
    );
}
