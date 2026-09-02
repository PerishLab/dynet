use dynet_api::outbound::veil;
use std::env;
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

fn secret() -> Option<[u8; 16]> {
    let told = env::var("DYNET_EXIT_SECRET").ok()?;
    let mut drawn = [0u8; 16];
    let raw = unbase(&told)?;
    if raw.len() != 16 {
        return None;
    }
    drawn.copy_from_slice(&raw);
    Some(drawn)
}

fn unbase(text: &str) -> Option<Vec<u8>> {
    const CODE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut held = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for byte in text.bytes().filter(|item| *item != b'=') {
        let place = CODE.iter().position(|item| *item == byte)? as u32;
        held = (held << 6) | place;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((held >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

#[test]
#[ignore]
fn reaches() {
    let secret = secret().expect("set DYNET_EXIT_SECRET to the exit's key");
    let seat = env::var("DYNET_EXIT_SEAT").expect("set DYNET_EXIT_SEAT to host:port");
    let mut stream = TcpStream::connect(&seat).expect("the exit refused the connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("timeout");
    let (mut cloak, mut shroud) = veil(secret).expect("shroud");
    cloak
        .greet(
            &mut stream,
            ("api.ipify.org", 80),
            b"GET /?format=text HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n",
        )
        .expect("greeting");
    stream.flush().expect("flush");
    let mut answer = Vec::new();
    while let Some(part) = shroud.receive(&mut stream).expect("response") {
        answer.extend_from_slice(&part);
        if answer.len() > 4096 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&answer);
    let egress = text.rsplit("\r\n\r\n").next().unwrap_or_default().trim();
    println!("the exit speaks from {egress}");
    assert!(text.starts_with("HTTP/1.1 200"), "no answer: {text:.160}");
    assert!(!egress.is_empty(), "the answer carried no address");
}
