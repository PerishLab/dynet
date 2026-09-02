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
    assert_eq!(carriages, [Carriage::Relay, Carriage::Absent]);
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

#[test]
fn spoken() {
    let text = concat!(
        "proxies:\n",
        "  - {name: one, type: vmess, server: a.example.com, port: 1, uuid: u, udp: true}\n",
        "  - {name: two, type: vmess, server: b.example.com, port: 2, uuid: u}\n",
        "  - {name: exit, type: vmess, server: c.example.com, port: 3, uuid: u}\n",
    );
    let entries = subscription::read(text).expect("entries");
    let spoken = concat!(
        "[[cluster]]\nname = \"near\"\nnodes = [\"one\", \"two\"]\n\n",
        "[[cluster]]\nname = \"pinned\"\nvia = \"near\"\nnodes = [\"exit\"]\n",
    );
    let held = catalog::declare(spoken, &entries).expect("declaration");
    assert_eq!(held.clusters().len(), 2);
    let pinned = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == "pinned")
        .expect("the detour");
    assert_eq!(pinned.via().expect("via").get(), "near");
    assert!(
        !held
            .clusters()
            .iter()
            .any(|item| item.name().get() == "near" && item.via().is_some()),
        "an ordinary cluster names no route to itself"
    );
}

#[test]
fn refused() {
    let text = "proxies:\n  - {name: one, type: vmess, server: a.example.com, port: 1, uuid: u}\n";
    let entries = subscription::read(text).expect("entries");
    let absent = "[[cluster]]\nname = \"near\"\nnodes = [\"missing\"]\n";
    let error = catalog::declare(absent, &entries).expect_err("an undeclared node must refuse");
    assert!(error.to_string().contains("does not carry"), "{error}");

    let dangling = "[[cluster]]\nname = \"near\"\nvia = \"gone\"\nnodes = [\"one\"]\n";
    let error = catalog::declare(dangling, &entries).expect_err("a dangling route must refuse");
    assert!(error.to_string().contains("not declared"), "{error}");

    let chained = concat!(
        "[[cluster]]\nname = \"one\"\nvia = \"two\"\nnodes = [\"one\"]\n\n",
        "[[cluster]]\nname = \"two\"\nvia = \"three\"\nnodes = [\"one\"]\n\n",
        "[[cluster]]\nname = \"three\"\nnodes = [\"one\"]\n",
    );
    let error = catalog::declare(chained, &entries).expect_err("two hops must refuse");
    assert!(error.to_string().contains("itself a detour"), "{error}");
}

#[test]
fn voiced() {
    let text = "proxies:\n  - {name: one, type: vmess, server: a.example.com, port: 1, uuid: u}\n";
    let entries = subscription::read(text).expect("entries");
    let spoken = concat!(
        "[[cluster]]\nname = \"near\"\nnodes = [\"one\"]\n\n",
        "[[cluster]]\nname = \"pinned\"\nvia = \"near\"\n",
        "[[cluster.node]]\nlabel = \"far\"\nserver = \"203.0.113.9\"\n",
        "port = 39127\nsecret = \"AAAAAAAAAAAAAAAAAAAAAA==\"\n",
    );
    let held = catalog::declare(spoken, &entries).expect("declaration");
    let pinned = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == "pinned")
        .expect("the detour");
    assert_eq!(pinned.nodes().len(), 1, "a declared node stands alone");
    assert_eq!(pinned.nodes()[0].label().get(), "far");
    assert!(
        !pinned.datagrams(),
        "an exit Dynet reaches by its stream alone carries no datagrams"
    );
    assert_eq!(
        held.spoken().len(),
        1,
        "the node it declared is handed back"
    );
    assert_eq!(held.spoken()[0].field("server"), Some("203.0.113.9"));
    let near = held
        .clusters()
        .iter()
        .find(|item| item.name().get() == "near")
        .expect("the first hop");
    assert_eq!(
        near.nodes().len(),
        1,
        "a declared node joins only its own cluster"
    );
}

#[test]
fn claimed() {
    let text = "proxies:\n  - {name: one, type: vmess, server: a.example.com, port: 1, uuid: u}\n";
    let entries = subscription::read(text).expect("entries");
    let spoken = concat!(
        "[[cluster]]\nname = \"near\"\nnodes = [\"one\"]\n\n",
        "[[cluster]]\nname = \"pinned\"\nvia = \"near\"\n",
        "[[cluster.node]]\nlabel = \"far\"\nserver = \"203.0.113.9\"\n",
        "port = 39127\nsecret = \"AAAAAAAAAAAAAAAAAAAAAA==\"\nudp = true\n",
    );
    let error = catalog::declare(spoken, &entries).expect_err("a claim must refuse");
    assert!(
        error.to_string().contains("cannot claim datagrams"),
        "{error}"
    );
}

#[test]
fn borne() {
    let text = concat!(
        "proxies:\n",
        "  - {name: one, type: vmess, server: a.example.com, port: 1, uuid: u, udp: true}\n",
        "  - {name: two, type: ss, server: b.example.com, port: 2, cipher: c, password: p, udp: true}\n",
    );
    let entries = subscription::read(text).expect("entries");
    let spoken = concat!(
        "[[cluster]]\nname = \"near\"\nnodes = [\"one\"]\n\n",
        "[[cluster]]\nname = \"other\"\nnodes = [\"two\"]\n",
    );
    let held = catalog::declare(spoken, &entries).expect("declaration");
    let carriage = |wanted: &str| {
        held.clusters()
            .iter()
            .find(|item| item.name().get() == wanted)
            .expect("the cluster")
            .nodes()[0]
            .capability()
            .carriage()
    };
    assert_eq!(
        carriage("near"),
        Carriage::Relay,
        "a subscription protocol Dynet speaks carries datagrams over its stream"
    );
    assert_eq!(
        carriage("other"),
        Carriage::Absent,
        "a vendor flag on a protocol Dynet does not speak claims nothing"
    );
}
