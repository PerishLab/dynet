use dynet_core::{
    Affinity, Bearing, Capability, Carriage, Cluster, Label, Name, Node, Pool, Spread,
};
use std::time::Duration;

fn node(label: &str, carriage: Carriage) -> Node {
    Node::new(
        Label::new(label).expect("node label"),
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
    assert!(cluster(&[Carriage::Relay, Carriage::Relay]).datagrams());
    assert!(
        cluster(&[Carriage::Relay, Carriage::Absent]).datagrams(),
        "one node that carries them is enough, because selection chooses within those that do"
    );
    assert!(
        !cluster(&[Carriage::Absent, Carriage::Absent]).datagrams(),
        "a cluster with no bearer at all carries none"
    );
}

#[test]
fn bearers() {
    let held = cluster(&[Carriage::Relay, Carriage::Absent]);
    assert_eq!(held.bearers(Bearing::Stream).len(), 2, "every node streams");
    let borne = held.bearers(Bearing::Datagram);
    assert_eq!(borne.len(), 1, "only the declared bearer takes datagrams");
    assert_eq!(borne[0].label().get(), "node-0");
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
