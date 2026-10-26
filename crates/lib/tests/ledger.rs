use dynet_core::{Domain, Ledger, Name};
use std::collections::BTreeMap;
use std::net::IpAddr;

const FORMAT: &str = r#"[{"address":"203.0.113.1","names":["a.example.com"],"cluster":"tunnel","issued":1000,"expires":1060,"retained":87400,"crowded":false,"released":false}]"#;

fn name(value: &str) -> Name {
    Name::new(value).expect("cluster name")
}

fn domain(value: &str) -> Domain {
    Domain::new(value).expect("domain")
}

fn address(value: &str) -> IpAddr {
    value.parse().expect("address")
}

#[test]
fn retains() {
    let mut ledger = Ledger::new(0);
    let held = address("203.0.113.1");
    assert!(ledger.record(
        held,
        (&domain("a.example.com"), &name("tunnel")),
        (60, 1000)
    ));
    let issue = ledger.lookup(held).expect("issued");
    assert_eq!((issue.expires(), issue.retained()), (1060, 1180));
    assert!(
        ledger.sweep(1179).is_empty(),
        "TTL plus 120s holds the address"
    );
    assert_eq!(ledger.sweep(1180), vec![held]);
    let mut floored = Ledger::new(86_400);
    floored.record(
        held,
        (&domain("a.example.com"), &name("tunnel")),
        (60, 1000),
    );
    assert_eq!(floored.lookup(held).expect("issued").retained(), 87_400);
    assert!(
        floored.sweep(87_399).is_empty(),
        "the floor outlasts the TTL"
    );
}

#[test]
fn engaged() {
    let mut ledger = Ledger::new(0);
    let held = address("203.0.113.1");
    ledger.record(
        held,
        (&domain("a.example.com"), &name("tunnel")),
        (60, 1000),
    );
    ledger.engage(&BTreeMap::from([(held, 2)]));
    assert_eq!(ledger.lookup(held).expect("issued").sessions(), 2);
    assert!(
        ledger.sweep(5000).is_empty(),
        "a live session pins the address"
    );
    ledger.engage(&BTreeMap::new());
    assert_eq!(ledger.sweep(5000), vec![held]);
}

#[test]
fn named() {
    let mut ledger = Ledger::default();
    let held = address("203.0.113.1");
    for index in 0..33 {
        let told = domain(&format!("n{index}.example.com"));
        ledger.record(held, (&told, &name("tunnel")), (60, 1000));
    }
    let issue = ledger.lookup(held).expect("issued");
    assert_eq!(issue.names().len(), 32);
    assert!(issue.crowded(), "a thirty-third name crowds the address");
    assert_eq!(issue.domain().get(), "n32.example.com");
    ledger.record(
        held,
        (&domain("n5.example.com"), &name("tunnel")),
        (60, 2000),
    );
    let issue = ledger.lookup(held).expect("issued");
    assert_eq!(
        issue.names().len(),
        32,
        "a repeated name is moved, not doubled"
    );
    assert_eq!(issue.domain().get(), "n5.example.com");
}

#[test]
fn contested() {
    let mut ledger = Ledger::default();
    let shared = address("203.0.113.9");
    ledger.record(shared, (&domain("a.com"), &name("one")), (60, 0));
    ledger.record(shared, (&domain("b.com"), &name("one")), (60, 0));
    assert_eq!(
        ledger.contested(),
        0,
        "one cluster sharing an address is no contest"
    );
    ledger.record(shared, (&domain("c.com"), &name("two")), (60, 0));
    assert_eq!(ledger.contested(), 1);
    assert_eq!(
        ledger.lookup(shared).expect("issued").cluster(),
        &name("two")
    );
}

#[test]
fn unissued() {
    let mut ledger = Ledger::default();
    for kept in ["10.1.2.3", "192.168.1.1", "198.18.0.5", "127.0.0.1"] {
        assert!(
            !ledger.record(address(kept), (&domain("a.com"), &name("one")), (60, 0)),
            "{kept} is never issued"
        );
    }
    assert!(ledger.addresses().next().is_none());
}

#[test]
fn format() {
    let mut ledger = Ledger::default();
    ledger.record(
        address("203.0.113.1"),
        (&domain("a.example.com"), &name("tunnel")),
        (60, 1000),
    );
    assert_eq!(
        ledger.write(),
        FORMAT,
        "the ledger format only gains fields"
    );
    let read = Ledger::read(FORMAT, 0).expect("read");
    assert_eq!(read.write(), FORMAT);
    let elder = r#"[{"address":"203.0.113.1","names":["a.example.com"],"cluster":"tunnel","issued":1,"expires":2,"retained":3}]"#;
    assert_eq!(
        Ledger::read(elder, 0).expect("elder").addresses().count(),
        1
    );
    let later = r#"[{"address":"203.0.113.1","names":["a.example.com"],"cluster":"tunnel","issued":1,"expires":2,"retained":3,"crowded":false,"released":false,"future":7}]"#;
    assert_eq!(
        Ledger::read(later, 0).expect("later").addresses().count(),
        1,
        "unknown fields are kept readable"
    );
}

#[test]
fn unreadable() {
    assert!(Ledger::read("{", 0).is_err());
    assert!(Ledger::read(r#"[{"address":"203.0.113.1"}]"#, 0).is_err());
    let nameless = r#"[{"address":"203.0.113.1","names":[],"cluster":"tunnel","issued":1,"expires":2,"retained":3}]"#;
    assert!(Ledger::read(nameless, 0).is_err());
    let reserved = r#"[{"address":"10.0.0.1","names":["a.com"],"cluster":"tunnel","issued":1,"expires":2,"retained":3}]"#;
    assert!(
        Ledger::read(reserved, 0)
            .expect("read")
            .addresses()
            .next()
            .is_none(),
        "reserved addresses are never restored"
    );
}
