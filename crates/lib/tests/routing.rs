use dynet_core::{Domain, Name, Range, Rule, Subject, Table, spared};
use std::net::IpAddr;

fn name(value: &str) -> Name {
    Name::new(value).expect("cluster name")
}

fn domain(value: &str) -> Domain {
    Domain::new(value).expect("domain")
}

fn address(value: &str) -> IpAddr {
    value.parse().expect("address")
}

fn table() -> Table {
    let rules = vec![
        Rule::new(Subject::Exact(domain("news.example.com")), name("tunnel")),
        Rule::new(Subject::Suffix(domain("example.com")), name("hongkong")),
        Rule::new(
            Subject::Holds(Range::read("157.240.0.0/16").expect("range")),
            name("tunnel"),
        ),
    ];
    Table::new(rules, None).expect("table")
}

#[test]
fn normalizes() {
    assert_eq!(domain("EXAMPLE.com.").get(), "example.com");
    for refused in ["", ".", "a..b", "bad domain"] {
        assert!(Domain::new(refused).is_err(), "{refused} must refuse");
    }
}

#[test]
fn labels() {
    assert!(domain("a.example.com").within(&domain("example.com")));
    assert!(domain("example.com").within(&domain("example.com")));
    assert!(
        !domain("notexample.com").within(&domain("example.com")),
        "a suffix must end on a label boundary"
    );
}

#[test]
fn ordering() {
    let table = table();
    assert_eq!(
        table.claimed(&domain("news.example.com")).map(Name::get),
        Some("tunnel"),
        "the first rule that matches decides"
    );
    assert_eq!(
        table.claimed(&domain("cdn.example.com")).map(Name::get),
        Some("hongkong")
    );
    assert_eq!(table.claimed(&domain("elsewhere.net")), None);
    assert_eq!(
        table.held(address("157.240.1.1")).map(Name::get),
        Some("tunnel")
    );
    assert_eq!(table.held(address("8.8.8.8")), None);
}

#[test]
fn prefixes() {
    let range = Range::read("10.1.0.0/12").expect("range");
    assert!(range.holds(address("10.15.255.255")));
    assert!(!range.holds(address("10.16.0.0")));
    assert!(!range.holds(address("::1")));
    assert!(Range::new(address("::"), 129).is_err());
    assert!(Range::read("10.0.0.0").is_err());
    let broad = Range::read("10.0.0.0/8").expect("range");
    assert!(broad.overlaps(range));
    assert!(range.overlaps(broad));
    assert!(!Range::read("11.0.0.0/8").expect("range").overlaps(range));
}

#[test]
fn masks() {
    for (told, base) in [
        ("203.0.113.199/24", "203.0.113.0"),
        ("203.0.113.199/0", "0.0.0.0"),
        ("203.0.113.199/32", "203.0.113.199"),
        ("2001:db8:1:2::9/48", "2001:db8:1::"),
        ("2001:db8::9/0", "::"),
        ("2001:db8::9/128", "2001:db8::9"),
    ] {
        let range = Range::read(told).expect("range");
        assert_eq!(range.masked().base(), address(base), "{told}");
        assert_eq!(range.masked().prefix(), range.prefix());
        assert_eq!(range.masked().masked(), range.masked());
    }
    for told in ["203.0.113.9", "2001:db8::9"] {
        let range = Range::host(address(told));
        assert!(range.holds(address(told)));
        assert_eq!(range.masked(), range);
    }
}

#[test]
fn single() {
    for (first, second) in [
        (
            Subject::Exact(domain("a.com")),
            Subject::Suffix(domain("a.com")),
        ),
        (
            Subject::Suffix(domain("a.com")),
            Subject::Suffix(domain("A.com.")),
        ),
        (
            Subject::Holds(Range::read("1.2.0.0/16").expect("range")),
            Subject::Holds(Range::read("1.2.0.0/16").expect("range")),
        ),
    ] {
        let rules = vec![
            Rule::new(first, name("one")),
            Rule::new(second, name("two")),
        ];
        let error = Table::new(rules, None).expect_err("a repeated name or prefix must refuse");
        assert!(error.to_string().contains("more than one rule"), "{error}");
    }
}

#[test]
fn spares() {
    for kept in [
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "224.0.0.0/4",
        "240.0.0.0/4",
    ] {
        let held = Range::read(kept).expect("range");
        assert!(spared(held.base()), "{kept} is reserved");
        let rules = vec![Rule::new(Subject::Holds(held), name("tunnel"))];
        assert!(Table::new(rules, None).is_err(), "holds {kept} must refuse");
    }
    let broad = vec![Rule::new(
        Subject::Holds(Range::read("0.0.0.0/0").expect("range")),
        name("tunnel"),
    )];
    assert!(
        Table::new(broad, None).is_err(),
        "a range enclosing a reserved one refuses"
    );
    assert!(!spared(address("8.8.8.8")));
}
