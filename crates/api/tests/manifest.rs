use dynet_api::manifest::load;
use dynet_api::outbound::{Front, Roster};
use dynet_core::Label;
use serde_json::{Value, json};

const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const SECRET: &str = "AAECAwQFBgcICQoLDA0ODw==";

fn base() -> Value {
    let reality = json!({"public_key": KEY, "short_id": "0a1b"});
    let tls = json!({"server_name": "cdn.example", "reality": reality});
    json!({
        "version": 1,
        "nodes": [
            {"label": "zx/tunnel", "protocol": "vless", "server": "192.0.2.10", "port": 443,
             "uuid": "6f1c2a3b-0000-4000-8000-000000000001", "flow": "xtls-rprx-vision", "tls": tls},
            {"label": "lisa/exit", "protocol": "shadowsocks", "server": "192.0.2.20", "port": 8388,
             "method": "2022-blake3-aes-128-gcm", "password": SECRET}
        ],
        "clusters": [
            {"name": "tunnel", "nodes": ["zx/tunnel"]},
            {"name": "veiled", "via": "tunnel", "exit": "lisa/exit"}
        ],
        "rules": [{"suffix": "example.com", "cluster": "veiled"}],
        "fallback": "direct"
    })
}

fn refused(change: impl FnOnce(&mut Value), needle: &str) {
    let mut value = base();
    change(&mut value);
    let error = load(&value.to_string()).expect_err(needle).to_string();
    assert!(error.contains(needle), "{needle} => {error}");
}

#[test]
fn speaks() {
    let loaded = load(&base().to_string()).expect("manifest");
    let front = &loaded.entries()[0];
    assert_eq!(front.field("name"), Some("zx/tunnel"));
    assert_eq!(front.field("type"), Some("vless"));
    assert_eq!(front.field("servername"), Some("cdn.example"));
    assert_eq!(front.field("reality-opts.public-key"), Some(KEY));
    assert_eq!(front.field("reality-opts.short-id"), Some("0a1b"));
    assert_eq!(front.field("flow"), Some("xtls-rprx-vision"));
    let exit = &loaded.entries()[1];
    assert_eq!(exit.field("type"), Some("ss"));
    assert_eq!(exit.field("cipher"), Some("2022-blake3-aes-128-gcm"));
    assert_eq!(exit.field("password"), Some(SECRET));
}

#[test]
fn vouches() {
    refused(
        |value| value["clusters"][0]["nodes"] = json!(["lisa/exit"]),
        "must speak vless or vmess",
    );
    refused(
        |value| value["nodes"][1]["method"] = json!("aes-128-gcm"),
        "must speak shadowsocks with 2022-blake3-aes-128-gcm",
    );
    refused(
        |value| value["nodes"][0]["tls"]["reality"]["public_key"] = json!("short"),
        "cannot front",
    );
    refused(
        |value| value["nodes"][1]["password"] = json!("AAEC"),
        "not sixteen",
    );
}

#[test]
fn boarding() {
    let label = Label::new("zx/tunnel").expect("label");
    let loaded = load(&base().to_string()).expect("manifest");
    assert!(matches!(
        Roster::new(loaded.entries(), 0).board(&label),
        Ok(Front::Vless(_))
    ));
    let mut value = base();
    value["nodes"][0]["protocol"] = json!("vmess");
    let loaded = load(&value.to_string()).expect("VMess manifest");
    assert!(matches!(
        Roster::new(loaded.entries(), 0).board(&label),
        Ok(Front::Vmess(_))
    ));
}

#[test]
fn authentication() {
    let loaded = load(&base().to_string()).expect("manifest");
    let label = Label::new("zx/tunnel").expect("label");
    let fields = [
        ("name", "zx/tunnel"),
        ("type", "vless"),
        ("server", "127.0.0.1"),
        ("port", "443"),
        ("uuid", "6f1c2a3b-0000-4000-8000-000000000001"),
    ]
    .map(|(key, value)| (key.to_owned(), value.to_owned()));
    let incomplete = dynet_api::Entry::shaped(fields.into_iter().collect());
    assert!(Roster::new(&[incomplete], 0).board(&label).is_err());
    let unknown = Label::new("unknown/front").expect("unknown label");
    assert!(Roster::new(loaded.entries(), 0).board(&unknown).is_err());
}
