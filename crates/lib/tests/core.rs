use dynet_core::{Affinity, Capability, Carriage, Cluster, Name, Node, Pool, Spread};
use std::time::Duration;

fn node(name: &str, carriage: Carriage) -> Node {
    Node::new(
        Name::new(name).expect("node name"),
        Capability::new(carriage),
    )
}

fn cluster(carriages: &[Carriage]) -> Cluster {
    let nodes = carriages
        .iter()
        .enumerate()
        .map(|(index, carriage)| node(&format!("node-{index}"), *carriage))
        .collect();
    Cluster::new(Name::new("provider").expect("cluster name"), nodes).expect("cluster")
}

#[test]
fn names() {
    assert_eq!(Name::new("hk").expect("name").get(), "hk");
    for refused in ["", "   ", "two words"] {
        assert!(Name::new(refused).is_err(), "{refused} must refuse");
    }
}

#[test]
fn empty() {
    let error = Cluster::new(Name::new("provider").expect("name"), Vec::new())
        .expect_err("an empty cluster must refuse");
    assert!(error.to_string().contains("no node"), "{error}");
}

#[test]
fn carriage() {
    assert!(Carriage::Native.datagrams());
    assert!(Carriage::Associate.datagrams());
    assert!(Carriage::Relay.datagrams());
    assert!(!Carriage::Absent.datagrams());
}

#[test]
fn weakest() {
    assert!(cluster(&[Carriage::Native, Carriage::Relay]).datagrams());
    assert!(
        !cluster(&[Carriage::Native, Carriage::Absent]).datagrams(),
        "one node without datagrams must disqualify the cluster it spreads across"
    );
}

#[test]
fn spreading() {
    let wide = Pool::wide(cluster(&[Carriage::Native]));
    assert_eq!(wide.spread(), Spread::Wide);
    assert!(wide.affinity().is_none());

    let window = Affinity::new(Duration::from_secs(300)).expect("affinity");
    let bound = Pool::bound(cluster(&[Carriage::Native]), window);
    assert_eq!(bound.spread(), Spread::Bound);
    assert_eq!(bound.affinity().expect("window").window().as_secs(), 300);

    assert!(Affinity::new(Duration::ZERO).is_err());
}
