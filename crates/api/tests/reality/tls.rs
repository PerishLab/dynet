use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use dynet_api::outbound::Reality;
use dynet_api::outbound::vless::certificate::vouch;
use dynet_api::outbound::vless::handshake::authorize;
use dynet_api::outbound::vless::hello::{Shares, build};
use dynet_api::outbound::vless::record::Cipher;
use dynet_api::outbound::vless::schedule::{Hash, Ladder};
use x25519_dalek::{PublicKey, StaticSecret};

const LEAF: &str = "3081ad3061a003020102020101300506032b657030003020170d3030303130313030303030305a180f32313030303130313030303030305a3000302a300506032b6570032100fd1724385aa0c75b64fb78cd602fa1d991fdebf76b13c58ed702eac835e9f618300506032b65700341002d2b97b5607c3ddbce5069ba22d4c54f1f4c072bd60de6e61b023f5c500f7f8cbf9d586a16a218605019dbeebea83289dcf3584e11d7aeddf7ba778b5a15ff5b";

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|place| u8::from_str_radix(&text[place..place + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn early() {
    let hash = Hash::Short;
    let early = hash.extract(&[], &[0u8; 32]);
    assert_eq!(
        early,
        hex("33ad0a1c607ec03b09e6cd9893680ce210adf300aa1f2660e1b22e10f170f92a")
    );
    assert_eq!(
        hash.derive(&early, "derived", &[]),
        hex("6f2615a108c702c5678f54fc9dbab69716c076189c48250cebeac3576c3611ba")
    );
}

#[test]
fn ladder() {
    let shared: Vec<u8> = (0..32).collect();
    let transcript = b"dynet transcript";
    let mut later = transcript.to_vec();
    later.push(b'!');
    let short = Ladder::climb(Hash::Short, &shared, transcript);
    assert_eq!(
        short.client,
        hex("c4ab71bfd8fc2b8ea8a698f64f43945ffc79db45084933526dcd76d02ea4cf13")
    );
    assert_eq!(
        short.server,
        hex("89d0cf100fe8aa50859bfccc02800485fef49578dd14d4fb2fe57d3e69e7c163")
    );
    assert_eq!(
        short.settle(&later).0,
        hex("daa30ead21e506a034182623a523abc24a9b269aa2429f5aeb2a60c9f25884c1")
    );
    assert_eq!(
        Hash::Short.finish(&short.server, transcript),
        hex("407541ec6773f92ce95707c32aa357f09bd6ba37825321c4ef45760064791624")
    );
    let long = Ladder::climb(Hash::Long, &shared, transcript);
    assert_eq!(
        long.client,
        hex(
            "784bd17e09c2c0b7ebf8eaf7d784fe95b657a45d267bcb6cee10f839835900f7906803cb242651cf13365275202b0581"
        )
    );
    assert_eq!(
        long.settle(&later).0,
        hex(
            "13d8548a8a4488277d828fafeb18974bf9a3fd6b47970becbae99ec34a6a2cd624cc0d8bfac48c60ca62709d22292b12"
        )
    );
}

struct Case {
    suite: u16,
    size: usize,
    first: &'static str,
    second: &'static str,
}

#[test]
fn records() {
    let cases = [
        Case {
            suite: 0x1301,
            size: 32,
            first: "1703030016f10774a2b29c474bd27fdd4e748d5058d6205309cd58",
            second: "17030300161d90a9df5f8b67e98026f60909bdc2b77a0102e0730d",
        },
        Case {
            suite: 0x1302,
            size: 48,
            first: "1703030016ed1280089cb2d9402bb1db83f71a6abd072c04f6ecb5",
            second: "17030300163423ea7dfc21fc44aa0dd88b3603c4156ca015dff472",
        },
        Case {
            suite: 0x1303,
            size: 32,
            first: "170303001632c2d173d976c3168b8c1cd38c598c61a26f1a6c431b",
            second: "170303001656369f31a7d3eaf317bd603b4bba1bb3e0f1957cd71c",
        },
    ];
    for Case {
        suite,
        size,
        first,
        second,
    } in cases
    {
        let mut sealer = Cipher::new(suite, vec![7u8; size]).expect("cipher");
        assert_eq!(sealer.seal(23, b"hello").expect("seal"), hex(first));
        assert_eq!(sealer.seal(23, b"again").expect("seal"), hex(second));
        let mut opener = Cipher::new(suite, vec![7u8; size]).expect("cipher");
        let sealed = hex(first);
        let (kind, body) = opener.open(&sealed[..5], &sealed[5..]).expect("open");
        assert_eq!((kind, body.as_slice()), (23, &b"hello"[..]));
        let forged = hex(second);
        assert!(opener.open(&forged[..5], &forged[6..]).is_err());
    }
}

#[test]
fn certificate() {
    let leaf = hex(LEAF);
    let key = vouch(&leaf, &[5u8; 32]).expect("the REALITY signature holds");
    assert_eq!(
        key.to_vec(),
        hex("fd1724385aa0c75b64fb78cd602fa1d991fdebf76b13c58ed702eac835e9f618")
    );
    let error = vouch(&leaf, &[6u8; 32]).expect_err("a foreign key must refuse");
    assert!(error.to_string().contains("REALITY"), "{error}");
}

fn reality(server: &StaticSecret) -> Reality {
    Reality {
        host: "192.0.2.1".to_string(),
        port: 443,
        identity: [1u8; 16],
        name: "example.com".to_string(),
        key: PublicKey::from(server).to_bytes(),
        short: [0xab, 0xcd, 0, 0, 0, 0, 0, 0],
        vision: true,
        mark: 0,
    }
}

#[test]
fn session() {
    let server = StaticSecret::from([3u8; 32]);
    let client = StaticSecret::from([4u8; 32]);
    let seed = [6u8; 32];
    let mut message = vec![9u8; 120];
    message[39..71].fill(0);
    let pristine = message.clone();
    let auth = authorize(&client, &reality(&server), &seed, &mut message).expect("seal");
    assert_eq!(
        auth,
        hex("b682aebd61686950f9b9c2e24d478667bedc207e8dc9a29b8153f1713cdce31a")
    );
    let cipher = Aes256Gcm::new_from_slice(&auth).expect("key");
    let plain = cipher
        .decrypt(
            seed[20..].into(),
            Payload {
                msg: &message[39..71],
                aad: &pristine,
            },
        )
        .expect("the server opens the session id");
    assert_eq!(&plain[..4], &[25, 9, 11, 0]);
    assert_eq!(&plain[8..], &[0xab, 0xcd, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&message[..39], &pristine[..39]);
    assert_eq!(&message[71..], &pristine[71..]);
}

#[test]
fn hello() {
    let shares = Shares {
        curve: [2u8; 32],
        lattice: vec![8u8; 1184],
    };
    let message = build(&[1u8; 32], "example.com", &shares).expect("hello");
    let size =
        usize::from(message[1]) << 16 | usize::from(message[2]) << 8 | usize::from(message[3]);
    assert_eq!((message[0], size), (1, message.len() - 4));
    assert_eq!(&message[4..6], &[3, 3]);
    assert_eq!(message[38], 32);
    assert!(message[39..71].iter().all(|byte| *byte == 0));
    let text = message.windows(11).any(|held| held == b"example.com");
    assert!(text, "the server name is carried");
    let mut hybrid = vec![0x11, 0xec, 0x04, 0xc0];
    hybrid.extend_from_slice(&[8u8; 16]);
    assert!(message.windows(hybrid.len()).any(|held| held == hybrid));
}
