use dynet_api::inbound::sniff::{self, Refused};

const SERVER: u16 = 0x0000;
const SPOKEN: u16 = 0x0010;
const ENCRYPTED: u16 = 0xfe0d;

fn wide(value: usize) -> [u8; 2] {
    u16::try_from(value)
        .expect("a width that fits")
        .to_be_bytes()
}

fn extension(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut held = kind.to_be_bytes().to_vec();
    held.extend_from_slice(&wide(body.len()));
    held.extend_from_slice(body);
    held
}

fn named(name: &str) -> Vec<u8> {
    let mut entry = vec![0u8];
    entry.extend_from_slice(&wide(name.len()));
    entry.extend_from_slice(name.as_bytes());
    let mut held = wide(entry.len()).to_vec();
    held.extend_from_slice(&entry);
    extension(SERVER, &held)
}

fn hello(extensions: &[u8]) -> Vec<u8> {
    let mut body = vec![0x01, 0, 0, 0];
    body.extend_from_slice(&[3, 3]);
    body.extend_from_slice(&[0u8; 32]);
    body.push(0);
    body.extend_from_slice(&[0, 2, 0x13, 0x01]);
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(&wide(extensions.len()));
    body.extend_from_slice(extensions);
    let width = u32::try_from(body.len() - 4).expect("a length that fits");
    body[1..4].copy_from_slice(&width.to_be_bytes()[1..]);
    body
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut held = vec![0x16, 3, 1];
    held.extend_from_slice(&wide(body.len()));
    held.extend_from_slice(body);
    held
}

#[test]
fn names() {
    let sniffed = sniff::read(&framed(&hello(&named("chatgpt.com")))).expect("a server name");
    assert_eq!(sniffed.name, "chatgpt.com");
    assert!(
        !sniffed.outer,
        "a hello without the extension names its own destination"
    );
    assert!(sniffed.alpn.is_empty());
}

#[test]
fn lowers() {
    let sniffed = sniff::read(&framed(&hello(&named("ChatGPT.COM")))).expect("a server name");
    assert_eq!(
        sniffed.name, "chatgpt.com",
        "a name is compared in one case"
    );
}

#[test]
fn encrypted() {
    let mut extensions = named("cloudflare-ech.com");
    extensions.extend_from_slice(&extension(ENCRYPTED, &[0x00, 0x01]));
    let sniffed = sniff::read(&framed(&hello(&extensions))).expect("a server name");
    assert_eq!(sniffed.name, "cloudflare-ech.com");
    assert!(
        sniffed.outer,
        "a name beside an encrypted client hello is the provider's and not the destination's"
    );
}

#[test]
fn spoken() {
    let mut extensions = named("crates.io");
    let told = [
        &[2u8, 0x68, 0x32][..],
        &[8, 0x68, 0x74, 0x74, 0x70, 0x2f, 0x31, 0x2e, 0x31][..],
    ]
    .concat();
    let mut body = wide(told.len()).to_vec();
    body.extend_from_slice(&told);
    extensions.extend_from_slice(&extension(SPOKEN, &body));
    let sniffed = sniff::read(&framed(&hello(&extensions))).expect("a server name");
    assert_eq!(sniffed.alpn, vec!["h2".to_string(), "http/1.1".to_string()]);
}

#[test]
fn wants() {
    let whole = framed(&hello(&named("claude.ai")));
    for short in [0, 3, 8, whole.len() - 1] {
        let told = sniff::read(&whole[..short]);
        let Err(Refused::Need(want)) = told else {
            panic!("a short hello must ask for a length, not refuse: {told:?}");
        };
        assert!(
            want > short,
            "a reader that asks for what it already holds cannot make progress"
        );
    }
    assert!(sniff::read(&whole).is_ok(), "the whole of it still reads");
}

#[test]
fn spans() {
    let body = hello(&named("api.openai.com"));
    let split = body.len() / 2;
    let mut held = framed(&body[..split]);
    held.extend_from_slice(&framed(&body[split..]));
    let sniffed = sniff::read(&held).expect("a name split across two records");
    assert_eq!(sniffed.name, "api.openai.com");
}

#[test]
fn refuses() {
    assert_eq!(
        sniff::read(b"GET / HTTP/1.1\r\n\r\n"),
        Err(Refused::Foreign)
    );
    assert_eq!(
        sniff::read(&framed(&hello(&named("trailing.example.")))),
        Err(Refused::Foreign),
        "a name carrying a trailing dot is refused rather than trimmed"
    );
    assert_eq!(
        sniff::read(&framed(&hello(&[]))),
        Err(Refused::Foreign),
        "a hello that names no server is nothing this can route on"
    );
}
