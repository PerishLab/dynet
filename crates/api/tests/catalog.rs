use dynet_api::{Reason, catalog, subscription};
use dynet_core::Carriage;

const SAMPLE: &str = "proxies:
  - {name: '\u{1F1ED}\u{1F1F0} one', type: vmess, server: a.invalid, port: 443, uuid: x, udp: true}
  - {name: '\u{1F1ED}\u{1F1F0} two', type: vmess, server: b.invalid, port: 443, uuid: y}
  - {name: '\u{1F1FA}\u{1F1F8} three', type: vmess, server: c.invalid, port: 443, uuid: z, udp: true}
  - {name: '\u{1F1FA}\u{1F1F8} four', type: ssr, server: d.invalid, port: 443, password: p, udp: true}
  - {name: 'Traffic: 7GB', type: vmess, server: e.invalid, port: 443, uuid: w, udp: true}
  - {name: 'Group', type: select, proxies: [one, two, three]}
";

fn catalog() -> dynet_api::Catalog {
    let entries = subscription::read(SAMPLE).expect("entries");
    catalog::build(&entries).expect("catalog")
}

#[test]
fn accounts() {
    let entries = subscription::read(SAMPLE).expect("entries");
    let catalog = catalog::build(&entries).expect("catalog");
    assert_eq!(entries.len(), 6);
    assert_eq!(
        catalog.nodes() + catalog.declined().len(),
        entries.len(),
        "every entry must be either a node or a declined one"
    );
}

#[test]
fn places() {
    let catalog = catalog();
    let names: Vec<_> = catalog
        .clusters()
        .iter()
        .map(|cluster| cluster.name().get().to_string())
        .collect();
    assert_eq!(names, ["hk", "us"]);
    assert_eq!(catalog.clusters()[0].nodes().len(), 2);
}

#[test]
fn reasons() {
    let catalog = catalog();
    let mut reasons: Vec<_> = catalog
        .declined()
        .iter()
        .map(|item| item.reason())
        .collect();
    reasons.sort_by_key(|reason| format!("{reason:?}"));
    assert_eq!(
        reasons,
        [Reason::Grouped, Reason::Placeless, Reason::Protocol],
        "a status entry carries no flag and is declined without matching vendor text"
    );
}

#[test]
fn declared() {
    let catalog = catalog();
    let hongkong = &catalog.clusters()[0];
    let carriages: Vec<_> = hongkong
        .nodes()
        .iter()
        .map(|node| node.capability().carriage())
        .collect();
    assert_eq!(carriages, [Carriage::Native, Carriage::Absent]);
    assert!(
        !hongkong.datagrams(),
        "one node without the flag must disqualify the cluster"
    );
}

#[test]
fn nesting() {
    let nested = "  - {name: 'x', type: vmess, ws-opts: {path: /a}}\n";
    let error = subscription::read(nested).expect_err("a nested mapping must refuse");
    assert!(error.to_string().contains("nested mapping"), "{error}");
}

#[test]
fn empty() {
    let error = subscription::read("mode: rule\n").expect_err("no proxy entry must refuse");
    assert!(error.to_string().contains("no proxy entry"), "{error}");
}
