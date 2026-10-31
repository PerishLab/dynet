use super::admission::Admission;
use super::call::Call;
use crate::control::Payload;
use crate::inbound::Held;

fn policy(suffix: &str) -> Held {
    let loaded = crate::manifest::load(
        &r#"{"version":1,"nodes":[{"label":"fixture/silent","protocol":"vmess","server":"127.0.0.1","port":1,"uuid":"6f1c2a3b-0000-4000-8000-000000000001","udp":true}],"clusters":[{"name":"proxy","nodes":["fixture/silent"]}],"rules":[{"suffix":"proxy.invalid","cluster":"proxy"}],"fallback":"direct"}"#.replace("proxy.invalid", suffix),
    ).unwrap();
    Held::new(
        Payload {
            loaded,
            upstream: "127.0.0.53".parse().unwrap(),
            retain: 86400,
        },
        None,
    )
    .unwrap()
}

fn ask(held: &Held, name: &str) -> Call {
    let framed = crate::resolver::ask(name, 1).unwrap();
    Call::new(held, framed[2..].to_vec(), "127.0.0.1:1".parse().unwrap()).unwrap()
}

#[test]
fn isolation() {
    let held = policy("proxy.invalid");
    let proxy = ask(&held, "node.proxy.invalid");
    let direct = ask(&held, "domestic.invalid");
    let admission = Admission::new();
    let permits: Vec<_> = (0..16).map(|_| admission.enter(&proxy).unwrap()).collect();
    assert!(admission.enter(&proxy).is_none());
    let direct = admission.enter(&direct).unwrap();
    drop(permits);
    assert!(admission.enter(&proxy).is_some());
    drop(direct);
}

#[test]
fn direct() {
    let held = policy("proxy.invalid");
    let call = ask(&held, "domestic.invalid");
    let admission = Admission::new();
    let permits: Vec<_> = (0..16).map(|_| admission.enter(&call).unwrap()).collect();
    assert!(admission.enter(&call).is_none());
    drop(permits);
    assert!(admission.enter(&call).is_some());
}

#[test]
fn snapshot() {
    let held = policy("proxy.invalid");
    let direct = ask(&held, "domestic.invalid");
    let proxy = ask(&held, "node.proxy.invalid");
    let later = policy("domestic.invalid");
    let claimed = ask(&later, "domestic.invalid");
    assert!(claimed.cluster.is_some());
    drop(held);
    assert!(direct.cluster.is_none());
    assert_eq!(proxy.cluster.unwrap().get(), "proxy");
}

#[test]
fn malformed() {
    let held = policy("proxy.invalid");
    assert!(Call::new(&held, vec![0; 12], "127.0.0.1:1".parse().unwrap()).is_err());
}
