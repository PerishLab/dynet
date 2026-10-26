use super::{base, read};
use dynet_core::{Bearing, Carriage, Form, Label, Name};
use serde_json::{Value, json};

fn name(value: &str) -> Name {
    Name::new(value).expect("name")
}

fn label(value: &str) -> Label {
    Label::new(value).expect("label")
}

#[test]
fn reads() {
    let manifest = read(&base()).expect("manifest");
    assert_eq!(manifest.generation(), Some("test"));
    let node = manifest.node(&label("zx.us-01/tunnel")).expect("node");
    assert_eq!(
        node.provider(),
        "zx.us-01",
        "a provider defaults to the label's host"
    );
    assert_eq!(node.protocol(), "vless");
    assert_eq!(node.field("tls.reality.public_key"), Some("key"));
    assert_eq!(node.field("port"), Some("443"));
    assert_eq!(node.field("label"), None, "spoken fields are not material");
    let relayed = manifest.node(&label("jm/one")).expect("node").capability();
    assert_eq!(relayed.carriage(), Carriage::Relay);
    let proxy = manifest.cluster(&name("proxy")).expect("proxy");
    assert_eq!(
        proxy.nodes().len(),
        2,
        "members gather every node of the provider"
    );
    assert_eq!(proxy.bearers(Bearing::Datagram).len(), 1);
    assert_eq!(proxy.bound().expect("bound").window().as_secs(), 600);
    let shared = manifest.pool(&name("shared")).expect("alias");
    assert_eq!(shared.name(), &name("proxy"), "an alias routes by its pool");
    let veiled = manifest.cluster(&name("veiled")).expect("veiled");
    assert_eq!(
        veiled.form(),
        &Form::Via(name("tunnel"), label("lisa.us-02/exit"))
    );
    assert!(manifest.cluster(&name("off")).expect("off").disabled());
    assert_eq!(
        manifest.table().fallback(),
        None,
        "direct is the absence of a cluster"
    );
}

#[test]
fn falls() {
    let mut value = base();
    value["fallback"] = json!("shared");
    let manifest = read(&value).expect("a cluster may be the fallback");
    assert_eq!(manifest.table().fallback(), Some(&name("shared")));
}

#[test]
fn drains() {
    let mut value = base();
    value["drain"] = json!({"nodes": ["jm/two"], "providers": ["zx.us-01"]});
    let manifest = read(&value).expect("manifest");
    assert_eq!(
        manifest
            .cluster(&name("proxy"))
            .expect("proxy")
            .nodes()
            .len(),
        1
    );
    assert!(
        manifest
            .cluster(&name("tunnel"))
            .expect("tunnel")
            .nodes()
            .is_empty()
    );
    assert!(manifest.drained(&label("zx.us-01/tunnel")));
}

fn refused(change: impl FnOnce(&mut Value), needle: &str) {
    let mut value = base();
    change(&mut value);
    let error = read(&value).expect_err(needle).to_string();
    assert!(error.contains(needle), "{needle} => {error}");
}

fn cluster(value: &mut Value, index: usize) -> &mut Value {
    &mut value["clusters"][index]
}

#[test]
fn refuses() {
    refused(|value| value["version"] = json!(2), "version 2");
    refused(
        |value| value["providers"] = json!([]),
        "unknown field `providers`",
    );
    refused(
        |value| value["nodes"][0]["label"] = json!("jm/one"),
        "declared twice",
    );
    refused(
        |value| {
            drop(
                value["nodes"][1]
                    .as_object_mut()
                    .map(|node| node.remove("protocol")),
            )
        },
        "carries no protocol",
    );
    refused(
        |value| cluster(value, 2)["nodes"] = json!(["jm/one"]),
        "exactly one",
    );
    refused(
        |value| cluster(value, 2)["bound"] = json!("1m"),
        "besides its pool",
    );
    refused(
        |value| cluster(value, 2)["disabled"] = json!(true),
        "besides its pool",
    );
    refused(
        |value| cluster(value, 2)["pool"] = json!("veiled"),
        "which is not a pool",
    );
    refused(
        |value| push(value, json!({"name": "deep", "pool": "shared"})),
        "which is not a pool",
    );
    refused(
        |value| {
            cluster(value, 0)
                .as_object_mut()
                .map(|pool| pool.remove("probe_exit"));
            cluster(value, 3)["via"] = json!("shared");
        },
        "which is not a pool",
    );
    refused(
        |value| {
            push(
                value,
                json!({"name": "twice", "via": "veiled", "exit": "lisa.us-02/exit"}),
            )
        },
        "which is not a pool",
    );
    refused(
        |value| cluster(value, 3)["exit"] = json!("nowhere/exit"),
        "named but not declared",
    );
    refused(
        |value| cluster(value, 3)["bound"] = json!("1m"),
        "is not a pool and cannot carry",
    );
    refused(
        |value| cluster(value, 0)["probe_exit"] = json!("jm/one"),
        "not the exit of any cluster",
    );
    refused(
        |value| cluster(value, 1)["bound"] = json!("ten"),
        "not a window",
    );
    refused(
        |value| cluster(value, 0)["name"] = json!("direct"),
        "cannot name a cluster",
    );
    refused(
        |value| cluster(value, 1)["name"] = json!("tunnel"),
        "declared twice",
    );
    refused(
        |value| cluster(value, 1)["members"] = json!([{"provider": "nobody"}]),
        "declares no node",
    );
    refused(
        |value| cluster(value, 0)["nodes"] = json!([]),
        "lists no node",
    );
    refused(
        |value| value["rules"][0]["suffix"] = json!("a.com"),
        "exactly one of exact, suffix and holds",
    );
    refused(
        |value| value["rules"][0]["cluster"] = json!("nowhere"),
        "named but not declared",
    );
    refused(
        |value| value["rules"][1]["suffix"] = json!("news.example.com"),
        "more than one rule",
    );
    refused(
        |value| value["rules"][2]["holds"] = json!("10.0.0.0/8"),
        "reserved",
    );
    refused(
        |value| value["fallback"] = json!("nowhere"),
        "named but not declared",
    );
    refused(
        |value| value["drain"] = json!({"nodes": ["nowhere/node"]}),
        "drain names node",
    );
}

fn push(value: &mut Value, cluster: Value) {
    if let Some(all) = value["clusters"].as_array_mut() {
        all.push(cluster);
    }
}
