use dynet_api::resolver::{self, Chain, Packet};
use dynet_core::Domain;

fn name(text: &str) -> Vec<u8> {
    resolver::ask(text, 1).unwrap()[14..]
        .split_last_chunk::<4>()
        .unwrap()
        .0
        .to_vec()
}

fn record(owner: &str, kind: u16, ttl: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = name(owner);
    bytes.extend(kind.to_be_bytes());
    bytes.extend(1u16.to_be_bytes());
    bytes.extend(ttl.to_be_bytes());
    bytes.extend((data.len() as u16).to_be_bytes());
    bytes.extend(data);
    bytes
}

fn reply(root: &str, answers: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let query = resolver::ask(root, 123).unwrap()[2..].to_vec();
    let mut body = query.clone();
    body[2] = 0x81;
    body[3] = 0xa0;
    body[6..8].copy_from_slice(&(answers.len() as u16).to_be_bytes());
    body.extend(answers.concat());
    (query, body)
}

#[test]
fn reachable() {
    let (query, body) = reply(
        "start.test",
        &[
            record("shared.test", 1, 900, &[8, 8, 4, 4]),
            record("start.test", 5, 5, &name("middle.test")),
            record("other.test", 1, 1500, &[9, 9, 9, 9]),
            record("middle.test", 5, 40, &name("shared.test")),
        ],
    );
    let mut chain = Chain::default();
    assert!(chain.extend(&query, &body, 40).unwrap().is_none());
    assert_eq!(chain.links[&Domain::new("shared.test").unwrap()], 45);
    let emitted = chain
        .project(&query, &Packet::new(&query).asked().unwrap())
        .unwrap();
    assert_eq!(emitted[3] & 0x20, 0);
    assert_eq!(emitted.len(), query.len() + 16);
    assert_eq!(resolver::read(&emitted).unwrap()[0].life, 5);
    assert_eq!(
        resolver::read(&emitted).unwrap()[0].address.octets(),
        [8, 8, 4, 4]
    );
}

#[test]
fn substitution() {
    let (query, body) = reply(
        "sub.origin.test",
        &[record("origin.test", 39, 4, &name("canonical.test"))],
    );
    let mut chain = Chain::default();
    let next = chain.extend(&query, &body, 90).unwrap().unwrap();
    assert_eq!(next.get(), "sub.canonical.test");
    let (query, body) = reply(next.get(), &[record(next.get(), 1, 50, &[8, 8, 8, 8])]);
    assert!(chain.extend(&query, &body, 92).unwrap().is_none());
    assert_eq!(chain.addresses[0].life, 2);
}

#[test]
fn invalid() {
    let mut chain = Chain::default();
    let (query, body) = reply(
        "a.test",
        &[
            record("a.test", 5, 8, &name("b.test")),
            record("b.test", 5, 8, &name("a.test")),
        ],
    );
    assert!(chain.extend(&query, &body, 2).is_err());
    let (query, body) = reply(
        "a.test",
        &[
            record("a.test", 5, 8, &name("b.test")),
            record("a.test", 5, 8, &name("c.test")),
        ],
    );
    assert!(Chain::default().extend(&query, &body, 2).is_err());
}
