use dynet_api::outbound::{Endpoint, Tunnel};
use std::env;

fn field(line: &str, name: &str) -> Option<String> {
    let key = format!("{name}: ");
    let start = line.find(&key)? + key.len();
    let rest = &line[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim().trim_matches('\'').to_string())
}

#[test]
#[ignore]
fn reaches() {
    let path = env::var("DYNET_SUBSCRIPTION").expect("set DYNET_SUBSCRIPTION");
    let want = env::var("DYNET_NODE").unwrap_or_else(|_| "0".to_string());
    let index: usize = want.parse().expect("index");
    let text = std::fs::read_to_string(path).expect("subscription");
    let entry = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("- {") && line.contains("type: vmess"))
        .filter(|line| field(line, "name").is_some_and(|n| n.starts_with('\u{1F1ED}')))
        .nth(index)
        .expect("a hongkong vmess node");
    let host = field(entry, "server").expect("server");
    let port: u16 = field(entry, "port").expect("port").parse().expect("port");
    let uuid = field(entry, "uuid").expect("uuid");

    let endpoint = Endpoint::new(host, port, &uuid, 0).expect("endpoint");
    let mut tunnel = Tunnel::open(&endpoint, "api.ipify.org", 80).expect("tunnel");
    tunnel
        .send(b"GET /?format=text HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n")
        .expect("request");
    let mut answer = Vec::new();
    while let Some(part) = tunnel.receive().expect("response") {
        answer.extend_from_slice(&part);
        if answer.len() > 4096 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&answer);
    let egress = text.rsplit("\r\n\r\n").next().unwrap_or_default().trim();
    println!("node index {index} -> egress {egress}");
    assert!(text.starts_with("HTTP/1.1 200"), "no answer: {text:.120}");
    assert!(!egress.is_empty(), "the answer carried no address");
}
