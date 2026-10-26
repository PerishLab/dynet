use dynet_core::{Affinity, Bearing, Capability, Carriage, Cluster, Form, Label, Name, Node};
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
    Cluster::pool(Name::new("provider").expect("cluster name"), nodes).expect("cluster")
}

#[test]
fn names() {
    assert_eq!(Name::new("hk").expect("name").get(), "hk");
    for refused in ["", "   ", "two words"] {
        assert!(Name::new(refused).is_err(), "{refused} must refuse");
    }
}

#[test]
fn reserved() {
    let error = Cluster::pool(Name::new("direct").expect("name"), Vec::new())
        .expect_err("direct is not a cluster");
    assert!(
        error.to_string().contains("leaving from the host"),
        "{error}"
    );
    let looped = Cluster::new(
        Name::new("alias").expect("name"),
        Form::Alias(Name::new("alias").expect("name")),
    );
    assert!(looped.is_err(), "a cluster cannot lead to itself");
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
fn windows() {
    assert_eq!(
        Affinity::read("30s").expect("seconds").window().as_secs(),
        30
    );
    assert_eq!(
        Affinity::read("10m").expect("minutes").window().as_secs(),
        600
    );
    assert_eq!(
        Affinity::read("1h").expect("hours").window().as_secs(),
        3600
    );
    for refused in ["", "0s", "10", "m", "5d"] {
        assert!(Affinity::read(refused).is_err(), "{refused} must refuse");
    }
    let bound = cluster(&[Carriage::Native]).bounded(Affinity::read("10m").expect("window"));
    assert_eq!(bound.bound().expect("bound").window().as_secs(), 600);
    assert!(Affinity::new(Duration::ZERO).is_err());
}
