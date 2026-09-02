use dynet_api::{Reason, catalog, subscription};
use std::collections::BTreeMap;
use std::env;
use std::fs;

fn survey() -> Option<(usize, dynet_api::Catalog)> {
    let path = env::var("DYNET_SUBSCRIPTION").ok()?;
    let text = fs::read_to_string(path).expect("subscription");
    let entries = subscription::read(&text).expect("entries");
    let catalog = catalog::build(&entries).expect("catalog");
    Some((entries.len(), catalog))
}

#[test]
#[ignore]
fn accounts() {
    let (entries, catalog) = survey().expect("set DYNET_SUBSCRIPTION to a saved subscription");
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for item in catalog.declined() {
        *reasons.entry(format!("{:?}", item.reason())).or_default() += 1;
    }
    println!("entries {entries}");
    println!("nodes {}", catalog.nodes());
    println!("clusters {}", catalog.clusters().len());
    for cluster in catalog.clusters() {
        println!(
            "  {:<4} nodes {:<3} datagrams {}",
            cluster.name().get(),
            cluster.nodes().len(),
            cluster.datagrams()
        );
    }
    println!("declined {reasons:?}");
    assert_eq!(
        catalog.nodes() + catalog.declined().len(),
        entries,
        "every entry must be either a node or a declined one"
    );
    assert!(
        catalog
            .declined()
            .iter()
            .any(|item| item.reason() == Reason::Placeless),
        "the provider's status entries must be declined for carrying no flag"
    );
}
