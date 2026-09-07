use dynet_core::{Decision, Domain, Ground, Name, Range, Router, Rule, Subject, Table};
use std::net::IpAddr;
use std::time::{Duration, Instant};

fn name(value: &str) -> Name {
    Name::new(value).expect("cluster name")
}

fn domain(value: &str) -> Domain {
    Domain::new(value).expect("domain")
}

fn table() -> Table {
    let rules = vec![
        Rule::new(Subject::Exact(domain("news.example.com")), name("direct")),
        Rule::new(Subject::Suffix(domain("example.com")), name("hongkong")),
        Rule::new(
            Subject::Holds(Range::new("10.0.0.0".parse().expect("base"), 8).expect("range")),
            name("direct"),
        ),
    ];
    Table::new(rules, name("default"))
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
    let exact = table.named(&domain("news.example.com"));
    assert_eq!(exact.cluster().get(), "direct");
    assert_eq!(exact.ground(), Ground::Named);
    assert_eq!(
        table.named(&domain("cdn.example.com")).cluster().get(),
        "hongkong"
    );
}

#[test]
fn unnamed() {
    let table = table();
    let unknown = table.named(&domain("elsewhere.net"));
    assert_eq!(unknown.cluster().get(), "default");
    assert_eq!(
        unknown.ground(),
        Ground::Default,
        "a decision must say what it rests on so blind traffic stays visible"
    );
}

#[test]
fn prefixes() {
    let range = Range::new("10.1.0.0".parse().expect("base"), 12).expect("range");
    assert!(range.holds("10.15.255.255".parse::<IpAddr>().expect("address")));
    assert!(!range.holds("10.16.0.0".parse::<IpAddr>().expect("address")));
    assert!(!range.holds("::1".parse::<IpAddr>().expect("address")));
    assert!(Range::new("::".parse().expect("base"), 129).is_err());
}

#[test]
fn carried() {
    let mut router = Router::new(table());
    let asked = router.asked(&domain("cdn.example.com"));
    assert_eq!(asked.cluster().get(), "hongkong");

    let answer: IpAddr = "203.0.113.9".parse().expect("answer");
    let now = Instant::now();
    router.issued(
        &domain("cdn.example.com"),
        &[answer],
        &asked,
        now + Duration::from_secs(60),
    );

    let reached = router.reached(answer, now);
    assert_eq!(reached.cluster().get(), "hongkong");
    assert_eq!(
        reached.ground(),
        Ground::Named,
        "a connection inherits the decision the answer was issued under"
    );
}

#[test]
fn expires() {
    let mut router = Router::new(table());
    let answer: IpAddr = "203.0.113.9".parse().expect("answer");
    let now = Instant::now();
    let decision = Decision::new(name("hongkong"), Ground::Named);
    router.issued(
        &domain("cdn.example.com"),
        &[answer],
        &decision,
        now + Duration::from_secs(1),
    );

    let stale = router.reached(answer, now + Duration::from_secs(2));
    assert_eq!(
        stale.ground(),
        Ground::Default,
        "an expired answer must fall back rather than keep a stale cluster"
    );
    assert_eq!(stale.cluster().get(), "default");
}

#[test]
fn contested() {
    let mut router = Router::new(table());
    let answer: IpAddr = "203.0.113.7".parse().expect("answer");
    let now = Instant::now();
    let later = now + Duration::from_secs(300);
    let sibling = Decision::new(name("hongkong"), Ground::Named);
    router.issued(&domain("one.example.com"), &[answer], &sibling, later);
    router.issued(&domain("two.example.com"), &[answer], &sibling, later);
    assert_eq!(
        router.ledger().contested(),
        0,
        "two names of one cluster sharing an address settle the same way twice"
    );

    let stranger = Decision::new(name("default"), Ground::Named);
    router.issued(&domain("three.example.com"), &[answer], &stranger, later);
    assert_eq!(
        router.ledger().contested(),
        1,
        "a shared address whose names want different clusters must be counted"
    );
    assert_eq!(
        router.reached(answer, now).cluster().get(),
        "default",
        "the latest answer still decides, because the connection carries no name"
    );
}

#[test]
fn released() {
    let mut router = Router::new(table());
    let early: IpAddr = "203.0.113.11".parse().expect("answer");
    let late: IpAddr = "203.0.113.12".parse().expect("answer");
    let now = Instant::now();
    let decision = Decision::new(name("hongkong"), Ground::Named);
    router.issued(
        &domain("brief.example.com"),
        &[early],
        &decision,
        now + Duration::from_secs(1),
    );
    router.issued(
        &domain("long.example.com"),
        &[late],
        &decision,
        now + Duration::from_secs(600),
    );

    let gone = router.forget(now + Duration::from_secs(2));
    assert_eq!(gone, vec![early], "an expired answer must name its address");
    assert!(
        router.forget(now + Duration::from_secs(2)).is_empty(),
        "a second sweep must find nothing left to release"
    );
    assert_eq!(
        router.reached(late, now + Duration::from_secs(2)).ground(),
        Ground::Named,
        "an answer still alive must survive the sweep"
    );
}

#[test]
fn holding() {
    let mut router = Router::new(table());
    let asked = router.asked(&domain("cdn.example.com"));
    let answer: IpAddr = "203.0.113.9".parse().expect("answer");
    let now = Instant::now();
    let life = Duration::from_secs(60);
    assert!(
        !router.holds(answer, now),
        "an address nobody was told about is carried by no issue"
    );
    router.issued(&domain("cdn.example.com"), &[answer], &asked, now + life);
    assert!(router.holds(answer, now));
    assert!(
        !router.holds(answer, now + life + Duration::from_secs(1)),
        "an issue past its expiry no longer carries the address it named"
    );
    let gone = router.forget(now + life + Duration::from_secs(1));
    assert_eq!(gone, vec![answer], "sweeping reports what it released");
    assert!(
        !router.holds(answer, now),
        "an address the sweep released is held by nothing, whatever the clock says"
    );
}
