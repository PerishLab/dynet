use super::*;
use crate::outbound::Passage;
use crate::resolver::{self, Packet};
use dynet_core::{Domain, Ledger, Sight};
use std::sync::Mutex;

fn wire(domain: &str, kind: u16, values: &[(u32, &[u8])]) -> Vec<u8> {
    let mut reply = resolver::ask(domain, 1).unwrap()[2..].to_vec();
    let end = reply.len() - 4;
    reply[end..end + 2].copy_from_slice(&kind.to_be_bytes());
    reply[2] = 0x81;
    reply[6..8].copy_from_slice(&(values.len() as u16).to_be_bytes());
    for &(ttl, bytes) in values {
        reply.extend([0xc0, 0x0c]);
        reply.extend(kind.to_be_bytes());
        reply.extend(1u16.to_be_bytes());
        reply.extend(ttl.to_be_bytes());
        reply.extend((bytes.len() as u16).to_be_bytes());
        reply.extend(bytes);
    }
    reply
}

fn record(question: Resolution<'_>, reply: Option<&[u8]>) -> Vec<Value> {
    let records = Mutex::new(Vec::new());
    dns(
        &|event| {
            let Evidence::Line(text) = event else {
                panic!("unexpected evidence loss")
            };
            assert!(text.len() <= 768);
            records
                .lock()
                .unwrap()
                .push(serde_json::from_str(text).unwrap());
        },
        question,
        reply,
    );
    records.into_inner().unwrap()
}

#[test]
fn answers() {
    let body = wire(
        "shop.example",
        1,
        &[(1, &[192, 0, 2, 1]), (19, &[192, 0, 2, 2])],
    );
    let query = Packet::new(&body).asked().unwrap();
    let suffix = Subject::Suffix(Domain::new("example").unwrap());
    let cluster = Name::new("fixtures").unwrap();
    let question = Resolution {
        generation: 12,
        query: &query,
        rule: Some(&suffix),
        cluster: Some(&cluster),
        source: "cache",
    };
    let rows = record(question, Some(&body));
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["rule"], "suffix");
    assert_eq!(rows[0]["suffix_labels"], 1);
    assert_eq!(rows[0]["source"], "cache");
    assert_eq!(rows[0]["result"], "answered");
    assert_eq!(rows[0]["binding_count"], 2);
    assert_eq!(rows[1]["ttl_s"], 1);
    assert_eq!(rows[2]["ttl_s"], 19);
    assert_eq!(rows[1]["address"], "192.0.2.1");
    for row in rows {
        assert_eq!(row["domain"], "shop.example");
        assert_eq!(row["policy"], "cluster");
        println!("{row}");
    }
}

#[test]
fn direct() {
    let ip: std::net::Ipv6Addr = "2001:db8::1".parse().unwrap();
    let body = wire("SHOP.EXAMPLE", 28, &[(0, &ip.octets())]);
    let query = Packet::new(&body).asked().unwrap();
    let question = Resolution {
        generation: 12,
        query: &query,
        rule: None,
        cluster: None,
        source: "upstream",
    };
    let rows = record(question, Some(&body));
    assert_eq!(rows[0]["rule"], "default");
    assert_eq!(rows[1]["domain"], "shop.example");
    assert_eq!(rows[1]["address"], "2001:db8::1");
    assert_eq!(rows[1]["ttl_s"], 0);
    assert_eq!(rows[1]["dns_id"], rows[0]["dns_id"]);
    assert_eq!(rows[1]["unix_ms"], rows[0]["unix_ms"]);
    assert_eq!(rows[0]["qtype"], 28);
}

#[test]
fn aliases() {
    fn name(text: &str) -> Vec<u8> {
        let query = resolver::ask(text, 1).unwrap();
        query[14..query.len() - 4].to_vec()
    }
    let mut body = wire("shop.example", 1, &[]);
    body[7] = 3;
    let mut append = |owner: &str, kind: u16, ttl: u32, bytes: &[u8]| {
        body.extend(name(owner));
        body.extend(kind.to_be_bytes());
        body.extend(1u16.to_be_bytes());
        body.extend(ttl.to_be_bytes());
        body.extend((bytes.len() as u16).to_be_bytes());
        body.extend(bytes);
    };
    append("other.example", 1, 200, &[192, 0, 2, 19]);
    append("answer.example", 1, 50, &[192, 0, 2, 2]);
    append("shop.example", 5, 2, &name("answer.example"));
    let query = Packet::new(&body).asked().unwrap();
    let rows = record(
        Resolution {
            generation: 12,
            query: &query,
            rule: None,
            cluster: None,
            source: "upstream",
        },
        Some(&body),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["address"], "192.0.2.2");
    assert_eq!(rows[1]["ttl_s"], 2);
}

#[test]
fn limits() {
    let domain = [
        &"a".repeat(63),
        &"b".repeat(63),
        &"c".repeat(63),
        &"d".repeat(61),
    ]
    .into_iter()
    .map(String::as_str)
    .collect::<Vec<_>>()
    .join(".");
    let ttl = (1, &[192, 0, 2, 1][..]);
    let body = wire(&domain, 1, &vec![ttl; 35]);
    let query = Packet::new(&body).asked().unwrap();
    let rule = Subject::Suffix(Domain::new(&domain).unwrap());
    let cluster = Name::new("a".repeat(2000)).unwrap();
    let rows = record(
        Resolution {
            generation: 12,
            query: &query,
            rule: Some(&rule),
            cluster: Some(&cluster),
            source: "cluster",
        },
        Some(&body),
    );
    assert_eq!(rows.len(), 33);
    assert_eq!(rows[0]["omitted"], 3);
    assert_eq!(rows[0]["binding_count"], 35);
    for row in rows {
        assert_eq!(row["domain"], domain);
        assert_eq!(row["labels_omitted"], true);
        assert!(row.get("cluster").is_none());
    }
}

#[test]
fn statuses() {
    let mut body = wire("test.example", 1, &[]);
    let query = Packet::new(&body).asked().unwrap();
    for (flags, code, result) in [
        (0x80, 0, "empty"),
        (0x82, 0, "truncated"),
        (0x80, 3, "negative"),
        (0x80, 5, "refused"),
        (0x80, 2, "failed"),
        (0x80, 1, "error"),
    ] {
        body[2] = flags;
        body[3] = code;
        let rows = record(
            Resolution {
                generation: 12,
                query: &query,
                rule: None,
                cluster: None,
                source: "local",
            },
            Some(&body),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["result"], result);
        assert_eq!(rows[0]["rcode"], code);
    }
    let rows = record(
        Resolution {
            generation: 12,
            query: &query,
            rule: None,
            cluster: None,
            source: "local",
        },
        None,
    );
    assert_eq!(rows[0]["result"], "unanswered");
    assert!(rows[0].get("rcode").is_none());
}

#[test]
fn attribution() {
    let loaded = crate::manifest::load(
        r#"{"version":1,"nodes":[],"clusters":[],"rules":[],"fallback":"direct"}"#,
    )
    .unwrap();
    let target = "192.0.2.1:443".parse::<SocketAddrV4>().unwrap();
    let chosen = super::super::Chosen {
        charge: None,
        told: "direct".into(),
        exit: None,
        passage: Passage::Direct(0),
    };
    for (told, bearing, scope) in [
        (
            Told::read("presented.example", false),
            Bearing::Stream,
            "flow",
        ),
        (Told::read("outer.example", true), Bearing::Stream, "flow"),
        (None, Bearing::Datagram, "packet_sample"),
    ] {
        let sight = Sight {
            address: (*target.ip()).into(),
            told,
            bearing,
        };
        let decision = dynet_core::decide(loaded.manifest(), &Ledger::new(86400), &sight);
        let selection = Selection {
            generation: 12,
            target,
            sight: &sight,
            decision: &decision,
            chosen: (bearing == Bearing::Stream).then_some(&chosen),
            scope,
        };
        route(
            Some(&|event| {
                let Evidence::Line(text) = event else {
                    panic!("unexpected evidence loss")
                };
                let row: Value = serde_json::from_str(text).unwrap();
                assert_eq!(row["address"], "192.0.2.1");
                assert_eq!(row["policy"], "direct");
                assert_eq!(
                    row["result"],
                    if bearing == Bearing::Stream {
                        "selected"
                    } else {
                        "refused"
                    }
                );
                assert_eq!(
                    row.get("observed_name").is_some(),
                    matches!(sight.told, Some(Told::Own(_)))
                );
                println!("{row}");
            }),
            selection,
        );
    }
}
