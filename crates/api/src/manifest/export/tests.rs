use super::{active, derive, render, same, serve};
use crate::control::{Payload, Private, Session};
use crate::host::State;
use dynet_core::Instance;
use secrets::{Envelope, ExposeSecret, Identity, Signer, Target};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FIRST: &str = "hex:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const SECOND: &str = "hex:1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100";
const PEER: &str = "hex:a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";

fn fixture(seed: &str, drained: bool) -> String {
    let drain = if drained {
        r#""drain": {"nodes": ["bandwagon/tunnel"]},"#
    } else {
        ""
    };
    format!(
        r#"{{"version":1,"upstream":"1.1.1.1","retain":86400,
"projection":{{"health_check_url":"http://la.vonet.online/generate_204","content_interval":1800,"path_interval":86400,"grace_slots":1,
"recipients":[{{"name":"mom","seed":"{seed}"}},{{"name":"spouse","seed":"{PEER}"}}]}},
"manifest":{{"version":1,"generation":"split-test","nodes":[
{{"label":"zxiyun/tunnel","protocol":"vless","server":"198.51.100.1","port":443,"uuid":"11111111-2222-3333-4444-555555555555","flow":"xtls-rprx-vision","tls":{{"server_name":"cdn.openbsd.org","reality":{{"public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","short_id":"01234567"}}}}}},
{{"label":"bandwagon/tunnel","protocol":"vless","server":"198.51.100.2","port":443,"uuid":"66666666-7777-8888-9999-aaaaaaaaaaaa","flow":"xtls-rprx-vision","tls":{{"server_name":"www.python.org","reality":{{"public_key":"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB","short_id":"89abcdef"}}}}}},
{{"label":"lisahost/sea","protocol":"shadowsocks","server":"198.51.100.3","port":39127,"method":"2022-blake3-aes-128-gcm","password":"MDEyMzQ1Njc4OWFiY2RlZg==","uot":2}}],
"clusters":[{{"name":"tunnel","nodes":["zxiyun/tunnel","bandwagon/tunnel"],"probe_exit":"lisahost/sea"}},{{"name":"L1","via":"tunnel","exit":"lisahost/sea"}},{{"name":"L3","pool":"tunnel"}}],
"rules":[{{"exact":"example.com","cluster":"L1"}},{{"suffix":"openai.com","cluster":"L1"}},{{"holds":"198.51.100.0/24","cluster":"L3"}}],
{drain}"fallback":"direct"}}}}"#
    )
}

#[test]
fn tokens() {
    let payload = Payload::check(fixture(FIRST, false).as_bytes()).unwrap();
    let spec = payload.projection.as_ref().unwrap();
    let mom = spec.recipient("mom").unwrap();
    let spouse = spec.recipient("spouse").unwrap();

    let first = derive(mom.seed(), 100);
    let repeat = derive(mom.seed(), 100);
    let next = derive(mom.seed(), 101);
    let peer = derive(spouse.seed(), 100);

    assert_eq!(first.len(), 43);
    assert_eq!(first, repeat);
    assert_ne!(first, next);
    assert_ne!(first, peer);
    assert!(same(&first, &repeat));
    assert!(!same(&first, &next));

    let redacted = format!("{spec:?} {mom:?}");
    assert!(!redacted.contains("00010203"));
    assert!(redacted.contains("[redacted]"));
    assert_eq!(active(spec, 100 * 86400 + 50).len(), 4);
}

#[test]
fn projection() {
    let payload = Payload::check(fixture(FIRST, false).as_bytes()).unwrap();
    let spec = payload.projection.as_ref().unwrap();

    let zero = render(&payload.loaded, Some(spec), 0).unwrap();
    let one = render(&payload.loaded, Some(spec), 1800).unwrap();

    assert!(zero.contains("dns:\n  enable: true\n  ipv6: false\n  enhanced-mode: fake-ip\n"));
    assert!(zero.contains("  nameserver:\n    - https://dns.alidns.com/dns-query\n"));
    assert!(zero.contains("  proxy-server-nameserver:\n    - 223.5.5.5\n"));
    assert!(zero.contains("udp-over-tcp: true"));
    assert!(zero.contains("udp-over-tcp-version: 2"));
    assert!(zero.contains("dialer-proxy: \"zxiyun/tunnel\""));
    assert!(zero.contains("dialer-proxy: \"bandwagon/tunnel\""));
    assert!(zero.contains("\"DOMAIN,example.com,L1\""));
    assert!(zero.contains("\"DOMAIN-SUFFIX,openai.com,L1\""));
    assert!(zero.contains("\"IP-CIDR,198.51.100.0/24,L3,no-resolve\""));
    assert!(zero.contains("\"MATCH,DIRECT\""));

    assert!(
        zero.find("- \"L1-zxiyun-sea\"").unwrap() < zero.find("- \"L1-bandwagon-sea\"").unwrap()
    );
    assert!(one.find("- \"L1-bandwagon-sea\"").unwrap() < one.find("- \"L1-zxiyun-sea\"").unwrap());

    let drained = Payload::check(fixture(FIRST, true).as_bytes()).unwrap();
    let pruned = render(&drained.loaded, drained.projection.as_ref(), 0).unwrap();
    assert!(!pruned.contains("bandwagon/tunnel"));
    assert!(pruned.contains("L1-zxiyun-sea"));
}

#[test]
fn invalid() {
    assert!(Payload::check(fixture("hex:01020304", false).as_bytes()).is_err());
    let bad = fixture(FIRST, false).replace("http://la.vonet.online/generate_204", "ftp://invalid");
    assert!(Payload::check(bad.as_bytes()).is_err());
}

fn fetch(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let code = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|num| num.parse::<u16>().ok())
        .unwrap_or(0);
    (code, response)
}

fn wait(addr: SocketAddr, path: &str, expected: u16) -> bool {
    for _ in 0..30 {
        if fetch(addr, path).0 == expected {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

#[test]
fn lifecycle() {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let home = builder.tempdir().unwrap();
    let root = home.path().canonicalize().unwrap().join("private");
    let private = Private::current();
    private.folder(&root).unwrap();
    let dir = root.join("state");
    private.folder(&dir).unwrap();

    let signer = Signer::generate().unwrap();
    let identity = Identity::generate();
    let target = Target::new("family-sub", "routing", "e1").unwrap();
    let key = root.join("identity.age");
    private
        .create(&key, identity.export().expose_secret().as_bytes())
        .unwrap();
    let envelope = root.join("routing.envelope");
    let first = fixture(FIRST, false);
    let sealed = Envelope::seal(
        target.clone(),
        1,
        first.as_bytes(),
        (&identity.recipient(), &signer),
    )
    .unwrap();
    private.create(&envelope, &sealed).unwrap();

    let bootstrap = root.join("bootstrap.json");
    let doc = serde_json::json!({
        "version": 1,
        "target": target,
        "identityRef": key,
        "publisher": signer.publisher().unwrap().to_string(),
        "minimum": 1,
        "source": {"kind": "file", "path": envelope}
    });
    private
        .create(&bootstrap, doc.to_string().as_bytes())
        .unwrap();

    let instance = Instance::new("dynetproj0").unwrap();
    let state = State::new(Some(dir)).unwrap();
    let session = Session::open(&bootstrap, &instance, &state, private).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let handle = std::thread::spawn(move || serve(listener, session, flag));

    let slot = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 86400;
    let parsed = Payload::check(first.as_bytes()).unwrap();
    let spec = parsed.projection.as_ref().unwrap();
    let mom = derive(spec.recipient("mom").unwrap().seed(), slot);
    let spouse = derive(spec.recipient("spouse").unwrap().seed(), slot);

    assert_eq!(fetch(addr, "/").0, 404);
    assert_eq!(fetch(addr, "/invalid").0, 404);
    let (code, body) = fetch(addr, &format!("/{mom}"));
    assert_eq!(code, 200);
    assert!(body.contains("Cache-Control: no-store"));
    assert!(body.contains("L1-zxiyun-sea"));
    assert_eq!(fetch(addr, &format!("/{spouse}")).0, 200);

    let second = fixture(SECOND, false);
    let rotated = Payload::check(second.as_bytes()).unwrap();
    let fresh = derive(
        rotated
            .projection
            .as_ref()
            .unwrap()
            .recipient("mom")
            .unwrap()
            .seed(),
        slot,
    );
    let next = Envelope::seal(
        target.clone(),
        2,
        second.as_bytes(),
        (&identity.recipient(), &signer),
    )
    .unwrap();
    private.replace(&envelope, &next).unwrap();

    assert!(wait(addr, &format!("/{fresh}"), 200));
    assert_eq!(fetch(addr, &format!("/{mom}")).0, 404);
    assert_eq!(fetch(addr, &format!("/{spouse}")).0, 200);

    let revoked = Envelope::revoke(target, 3, &signer).unwrap();
    private.replace(&envelope, &revoked).unwrap();
    assert!(wait(addr, &format!("/{fresh}"), 404));
    assert_eq!(fetch(addr, &format!("/{spouse}")).0, 404);

    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
