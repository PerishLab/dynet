mod decide;
mod read;

use dynet_core::Manifest;
use serde_json::{Value, json};

pub fn base() -> Value {
    let reality = json!({"public_key": "key", "short_id": "ab"});
    let tls = json!({"server_name": "cdn.example", "reality": reality});
    json!({
        "version": 1,
        "generation": "test",
        "nodes": [
            {"label": "zx.us-01/tunnel", "protocol": "vless", "server": "front.example", "port": 443,
             "uuid": "00000000-0000-0000-0000-000000000000", "flow": "xtls-rprx-vision",
             "tls": tls},
            {"label": "jm/one", "provider": "jm", "protocol": "vmess", "server": "one.example", "port": 1, "uuid": "u", "udp": true},
            {"label": "jm/two", "provider": "jm", "protocol": "vmess", "server": "two.example", "port": 2, "uuid": "u"},
            {"label": "lisa.us-02/exit", "protocol": "shadowsocks", "server": "exit.example", "port": 8388,
             "method": "2022-blake3-aes-128-gcm", "password": "secret"},
            {"label": "spare/node", "protocol": "vmess", "server": "spare.example", "port": 3, "uuid": "u"}
        ],
        "clusters": [
            {"name": "tunnel", "nodes": ["zx.us-01/tunnel"], "probe_exit": "lisa.us-02/exit"},
            {"name": "proxy", "members": [{"provider": "jm"}], "bound": "10m"},
            {"name": "shared", "pool": "proxy"},
            {"name": "veiled", "via": "tunnel", "exit": "lisa.us-02/exit"},
            {"name": "off", "nodes": ["spare/node"], "disabled": true}
        ],
        "rules": [
            {"exact": "news.example.com", "cluster": "veiled"},
            {"suffix": "example.com", "cluster": "shared"},
            {"holds": "157.240.0.0/16", "cluster": "veiled"},
            {"suffix": "off.test", "cluster": "off"}
        ],
        "fallback": "direct"
    })
}

pub fn read(value: &Value) -> Result<Manifest, dynet_core::Error> {
    Manifest::read(&value.to_string())
}
