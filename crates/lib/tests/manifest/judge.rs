use super::{base, read};
use dynet_core::{
    Bearing, Course, Domain, Ground, Ledger, Name, Range, Sieve, Sight, Tally, decide,
};
use std::net::IpAddr;

fn name(value: &str) -> Name {
    Name::new(value).expect("name")
}

fn address(value: &str) -> IpAddr {
    value.parse().expect("address")
}

fn issue(ledger: &mut Ledger, held: &str, told: (&str, &str)) {
    let domain = Domain::new(told.0).expect("domain");
    ledger.record(address(held), (&domain, &name(told.1)), (60, 1000));
}

fn sight(held: &str) -> Sight {
    Sight {
        told: None,
        address: address(held),
        bearing: Bearing::Stream,
    }
}

#[test]
fn rejudges() {
    let manifest = read(&base()).expect("manifest");
    let mut ledger = Ledger::new(0);
    issue(&mut ledger, "203.0.113.1", ("cdn.example.com", "renamed"));
    issue(&mut ledger, "203.0.113.2", ("x.other.net", "renamed"));
    issue(&mut ledger, "203.0.113.3", ("cdn.example.com", "shared"));
    for index in 0..33 {
        issue(
            &mut ledger,
            "203.0.113.4",
            (&format!("n{index}.other.net"), "renamed"),
        );
    }
    let tally = ledger.rejudge(&manifest);
    let expected = Tally {
        moved: 1,
        released: 1,
        kept: 0,
        crowded: 1,
    };
    assert_eq!(tally, expected);
    let moved = ledger.lookup(address("203.0.113.1")).expect("moved");
    assert_eq!(moved.cluster(), &name("shared"));
    let released = decide(&manifest, &ledger, &sight("203.0.113.2"));
    assert_eq!(released.course(), &Course::Direct);
    assert_eq!(released.ground(), Ground::Issued);
    let crowded = ledger.lookup(address("203.0.113.4")).expect("crowded");
    assert!(!crowded.released(), "a crowded address is never released");
    let again = Tally {
        crowded: 1,
        ..Tally::default()
    };
    assert_eq!(
        ledger.rejudge(&manifest),
        again,
        "only the crowded entry stays chosen"
    );
}

#[test]
fn flushes() {
    let manifest = read(&base()).expect("manifest");
    let mut ledger = Ledger::new(0);
    issue(&mut ledger, "203.0.113.1", ("cdn.example.com", "shared"));
    issue(&mut ledger, "203.0.113.2", ("news.example.com", "shared"));
    issue(&mut ledger, "203.0.113.3", ("gone.test", "shared"));
    issue(&mut ledger, "198.51.100.9", ("gone.test", "shared"));
    let sieve = Sieve::Prefix(Range::read("203.0.113.0/24").expect("range"));
    let before = ledger.write();
    let dry = ledger.flush(&manifest, &sieve, true);
    let expected = Tally {
        moved: 1,
        released: 1,
        kept: 1,
        crowded: 0,
    };
    assert_eq!(dry, expected);
    assert_eq!(ledger.write(), before, "a dry flush changes nothing");
    assert_eq!(ledger.flush(&manifest, &sieve, false), expected);
    let moved = ledger.lookup(address("203.0.113.2")).expect("moved");
    assert_eq!(moved.cluster(), &name("veiled"));
    assert_eq!(
        ledger.sweep(0),
        vec![address("203.0.113.3")],
        "released leaves once idle"
    );
    let named = Sieve::read("test").expect("sieve");
    assert_eq!(ledger.flush(&manifest, &named, false).released, 1);
}

#[test]
fn sieves() {
    assert_eq!(
        Sieve::read("203.0.113.1"),
        Some(Sieve::Address(address("203.0.113.1")))
    );
    assert!(matches!(
        Sieve::read("203.0.113.0/24"),
        Some(Sieve::Prefix(_))
    ));
    assert!(matches!(Sieve::read("example.com"), Some(Sieve::Name(_))));
    assert_eq!(Sieve::read("not a name"), None);
}
