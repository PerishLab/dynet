use dynet_api::inbound::table;
use dynet_api::{catalog, subscription};
use dynet_core::{Domain, Ground, Name};

const SAMPLE: &str = "proxies:
  - {name: '\u{1F1ED}\u{1F1F0} one', type: vmess, server: a.invalid, port: 443, uuid: x}
  - {name: '\u{1F1FA}\u{1F1F8} two', type: vmess, server: b.invalid, port: 443, uuid: y}
";

fn clusters() -> Vec<dynet_core::Cluster> {
    let entries = subscription::read(SAMPLE).expect("entries");
    catalog::build(&entries)
        .expect("catalog")
        .clusters()
        .to_vec()
}

#[test]
fn declares() {
    let held = clusters();
    let text = concat!(
        "fallback = \"direct\"\n\n",
        "[[rule]]\nsuffix = \"github.com\"\ncluster = \"hk\"\n\n",
        "[[rule]]\nexact = \"api.ipify.org\"\ncluster = \"us\"\n\n",
        "[[rule]]\nholds = \"38.55.133.71/32\"\ncluster = \"hk\"\n",
    );
    let table = table::declare(text, &held).expect("a table");
    assert_eq!(table.rules().len(), 3);
    assert_eq!(table.fallback().get(), "direct");

    let asked = Domain::new("api.github.com").expect("a domain");
    let decision = table.named(&asked);
    assert_eq!(decision.cluster().get(), "hk");
    assert_eq!(decision.ground(), Ground::Named);

    let exact = Domain::new("api.ipify.org").expect("a domain");
    assert_eq!(table.named(&exact).cluster().get(), "us");

    let deeper = Domain::new("one.api.ipify.org").expect("a domain");
    assert_eq!(
        table.named(&deeper).ground(),
        Ground::Default,
        "an exact subject must not carry what lies below it"
    );

    let held = "38.55.133.71".parse().expect("an address");
    assert_eq!(table.addressed(held).cluster().get(), "hk");
}

#[test]
fn defaults() {
    let held = clusters();
    let text = "[[rule]]\nsuffix = \"github.com\"\ncluster = \"hk\"\n";
    let table = table::declare(text, &held).expect("a table");
    assert_eq!(
        table.fallback().get(),
        "direct",
        "an undeclared fallback is direct"
    );
    let stray = Domain::new("example.invalid").expect("a domain");
    assert_eq!(table.named(&stray).ground(), Ground::Default);
}

#[test]
fn refuses() {
    let held = clusters();

    let empty = "fallback = \"direct\"\n";
    let error = table::declare(empty, &held).expect_err("an empty table must refuse");
    assert!(error.to_string().contains("at least one rule"), "{error}");

    let stray = "[[rule]]\nsuffix = \"github.com\"\ncluster = \"gone\"\n";
    let error = table::declare(stray, &held).expect_err("an undeclared cluster must refuse");
    assert!(error.to_string().contains("not declared"), "{error}");

    let bare = "[[rule]]\ncluster = \"hk\"\n";
    let error = table::declare(bare, &held).expect_err("a subjectless rule must refuse");
    assert!(error.to_string().contains("none of exact"), "{error}");

    let both = "[[rule]]\nsuffix = \"github.com\"\nexact = \"github.com\"\ncluster = \"hk\"\n";
    let error = table::declare(both, &held).expect_err("two subjects must refuse");
    assert!(error.to_string().contains("more than one"), "{error}");

    let loose = "[[rule]]\nsuffix = \"github.com\"\n";
    let error = table::declare(loose, &held).expect_err("a clusterless rule must refuse");
    assert!(error.to_string().contains("no cluster"), "{error}");

    let wrong = "fallback = \"gone\"\n\n[[rule]]\nsuffix = \"a.com\"\ncluster = \"hk\"\n";
    let error = table::declare(wrong, &held).expect_err("an undeclared fallback must refuse");
    assert!(error.to_string().contains("fallback names"), "{error}");
}

#[test]
fn relays() {
    let held = clusters();
    let first = table::declare("[[rule]]\nsuffix = \"a.com\"\ncluster = \"hk\"\n", &held)
        .expect("the first table");
    let mut router = dynet_core::Router::new(first);
    let asked = Domain::new("one.a.com").expect("a domain");
    assert_eq!(router.asked(&asked).cluster().get(), "hk");

    let second = table::declare("[[rule]]\nsuffix = \"a.com\"\ncluster = \"us\"\n", &held)
        .expect("the second table");
    router.relay(second);
    assert_eq!(
        router.asked(&asked).cluster().get(),
        "us",
        "a relayed table decides the next question"
    );
    assert_eq!(router.table().rules().len(), 1);
    assert_eq!(
        router.table().fallback(),
        &Name::new("direct").expect("a name")
    );
}
