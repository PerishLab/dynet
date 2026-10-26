use dynet_api::resolver::{prune, read};
use std::net::{IpAddr, Ipv4Addr};

fn question() -> Vec<u8> {
    let mut body = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 3, 0, 0, 0, 1];
    for label in ["www", "example", "com"] {
        body.push(u8::try_from(label.len()).expect("label"));
        body.extend_from_slice(label.as_bytes());
    }
    body.extend_from_slice(&[0, 0, 1, 0, 1]);
    body
}

fn reply() -> Vec<u8> {
    let mut body = question();
    body.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 6]);
    body.extend_from_slice(&[3, b'c', b'd', b'n', 0xc0, 16]);
    for address in [[10, 1, 2, 3], [203, 0, 113, 7]] {
        body.extend_from_slice(&[0xc0, 45, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        body.extend_from_slice(&address);
    }
    body.extend_from_slice(&[0, 0, 41, 4, 208, 0, 0, 0, 0, 0, 0]);
    body
}

fn private(address: Ipv4Addr) -> bool {
    dynet_core::spared(IpAddr::V4(address))
}

#[test]
fn keeps() {
    let found = read(&reply()).expect("answers");
    assert_eq!(found.len(), 2);
    let pruned = prune(&reply(), private)
        .expect("prune")
        .expect("a reserved address is left out");
    let kept: Vec<Ipv4Addr> = read(&pruned)
        .expect("answers")
        .iter()
        .map(|answer| answer.address)
        .collect();
    assert_eq!(kept, vec![Ipv4Addr::new(203, 0, 113, 7)]);
    assert_eq!(
        &pruned[6..12],
        &[0, 2, 0, 0, 0, 0],
        "the alias stays and extras go"
    );
    assert!(
        pruned.windows(3).any(|window| window == [3, b'c', b'd']),
        "names are spelled out so no pointer outlives what it pointed at"
    );
}

#[test]
fn untouched() {
    assert_eq!(prune(&reply(), |_| false).expect("prune"), None);
}

#[test]
fn emptied() {
    let pruned = prune(&reply(), |_| true).expect("prune").expect("pruned");
    assert!(read(&pruned).expect("answers").is_empty());
}
