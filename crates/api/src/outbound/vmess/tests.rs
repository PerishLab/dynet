use super::{Aead, Aes128Gcm, Block, Endpoint, Key, Nonce, Payload, kdf, seal};
use crate::inbound::{Link, Taken};
use crate::outbound::{Exit, Front, Passage, Reality};
use dynet_core::{Fault, Verdict};
use socket2::{Domain, Socket, Type};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const SECRET: &str = "86d424c7-f9a7-4c1d-b8b5-906a62502172";

fn chained(front: Front, host: String) -> Link {
    let exit = Exit {
        host: "exit.example".into(),
        port: 443,
        secret: [0x53; 16],
        relay: false,
    };
    Link::open(Passage::Veiled(front, exit), host, 443)
}

fn vmess(port: u16, host: String) -> Link {
    let endpoint = Endpoint::new("127.0.0.1", port, SECRET, 0).unwrap();
    chained(Front::Vmess(endpoint), host)
}

fn vless(port: u16) -> Link {
    let front = Reality {
        host: "127.0.0.1".into(),
        port,
        identity: [0x42; 16],
        name: "entry.example".into(),
        key: [0x31; 32],
        short: [0x61; 8],
        vision: false,
        mark: 0,
    };
    chained(Front::Vless(front), "target.example".into())
}

fn failed(link: &Link, expected: Fault, fields: &str) {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(8));
        if matches!(link.bide(Duration::from_millis(100)), Taken::Spent) {
            break;
        }
    }
    let line = std::cell::OnceCell::new();
    assert_eq!(
        link.report(&|text| line.set(text.to_owned()).unwrap()),
        Some(Verdict::Faulted(expected))
    );
    let line = line.into_inner().unwrap();
    assert!(line.starts_with(&format!(
        "request={} attempt=1 elapsed_ms=",
        link.request().unwrap()
    )));
    assert!(line.ends_with(fields), "{line}");
    assert!(line.len() < 256, "{line}");
    for secret in [
        SECRET,
        "entry.example",
        "exit.example",
        "target.example",
        "payload",
    ] {
        assert!(!line.contains(secret), "{line}");
    }
}

fn peer(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let bound = Duration::from_secs(4);
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((peer, _)) => {
                peer.set_nonblocking(false).unwrap();
                peer.set_read_timeout(Some(bound)).unwrap();
                peer.set_write_timeout(Some(bound)).unwrap();
                return peer;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                assert!(start.elapsed() < bound, "entry connection did not arrive");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

#[test]
fn concurrent() {
    let refused = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
    refused
        .bind(&SocketAddr::from(([127, 0, 0, 1], 0)).into())
        .unwrap();
    let port = refused.local_addr().unwrap().as_socket().unwrap().port();
    let connection = vmess(port, "target.example".into());
    let alert = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let handshake = vless(alert.local_addr().unwrap().port());
    let opening = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let exit = vmess(
        opening.local_addr().unwrap().port(),
        "target.example".into(),
    );
    let silent = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let timeout = vmess(silent.local_addr().unwrap().port(), "target.example".into());
    let mut entries = [
        connection.request(),
        handshake.request(),
        exit.request(),
        timeout.request(),
    ];
    entries.sort_unstable();
    assert!(entries.windows(2).all(|pair| pair[0] != pair[1]));

    let mut rejected = peer(&alert);
    rejected.write_all(b"\x15\x03\x03\x00\x07payload").unwrap();
    let mut opened = peer(&opening);
    acknowledge(&mut opened, SECRET);
    opened.shutdown(Shutdown::Write).unwrap();
    failed(
        &connection,
        Fault::Reach,
        "hop=entry stage=entry_connect last=none error=ConnectionRefused",
    );
    failed(
        &handshake,
        Fault::Handshake,
        "hop=entry stage=entry_handshake last=entry_connected error=operation_failed",
    );
    failed(
        &exit,
        Fault::Severed,
        "hop=exit stage=next_hop_open last=entry_handshake error=UnexpectedEof",
    );
    failed(
        &timeout,
        Fault::Silent,
        "hop=entry stage=entry_handshake last=entry_connected error=timeout",
    );
}

fn acknowledge(stream: &mut TcpStream, identity: &str) {
    let node = Endpoint::new("127.0.0.1", 0, identity, 0).unwrap();
    let command = node.command();
    let mut envelope = [0; 42];
    stream.read_exact(&mut envelope).unwrap();
    let auth = &envelope[..16];
    let nonce = &envelope[34..];
    let open = |labels: (&[u8], &[u8]), sealed: &[u8]| {
        let key = kdf::shorten(&command, &[labels.0, auth, nonce]);
        let seed = kdf::derive(&command, &[labels.1, auth, nonce]);
        Aes128Gcm::new(Key::<Aes128Gcm>::from_slice(&key))
            .decrypt(
                Nonce::from_slice(&seed[..12]),
                Payload {
                    msg: sealed,
                    aad: auth,
                },
            )
            .unwrap()
    };
    let length = open(
        (
            b"VMess Header AEAD Key_Length",
            b"VMess Header AEAD Nonce_Length",
        ),
        &envelope[16..34],
    );
    let size = usize::from(u16::from_be_bytes(length.try_into().unwrap()));
    let mut sealed = vec![0; size + 16];
    stream.read_exact(&mut sealed).unwrap();
    let header = open(
        (b"VMess Header AEAD Key", b"VMess Header AEAD Nonce"),
        &sealed,
    );
    let key = kdf::digest(&header[17..33]);
    let seed = kdf::digest(&header[1..17]);
    let reply = |labels: (&[u8], &[u8]), bytes: &[u8]| {
        seal(
            &kdf::shorten(&key[..16], &[labels.0]),
            &kdf::derive(&seed[..16], &[labels.1]),
            b"",
            bytes,
        )
        .unwrap()
    };
    let length = reply(
        (b"AEAD Resp Header Len Key", b"AEAD Resp Header Len IV"),
        &[0, 1],
    );
    let body = reply(
        (b"AEAD Resp Header Key", b"AEAD Resp Header IV"),
        &header[33..34],
    );
    stream.write_all(&length).unwrap();
    stream.write_all(&body).unwrap();
}
