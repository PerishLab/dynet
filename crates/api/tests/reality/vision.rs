use dynet_api::outbound::vless::filter::{Traffic, whole};
use dynet_api::outbound::vless::vision::{
    CONTINUE, DIRECT, END, block, reshape, settle, shape, unshape,
};

const IDENTITY: [u8; 16] = [0x5a; 16];

fn greeting() -> Vec<u8> {
    let mut body = vec![3, 3];
    body.extend_from_slice(&[7u8; 32]);
    body.push(32);
    body.extend_from_slice(&[8u8; 32]);
    body.extend_from_slice(&[0x13, 0x01, 0]);
    body.extend_from_slice(&[0, 6, 0x00, 0x2b, 0x00, 0x02, 0x03, 0x04]);
    let mut message = vec![2, 0, 0, u8::try_from(body.len()).expect("short")];
    message.extend_from_slice(&body);
    let mut record = vec![0x16, 3, 3, 0, u8::try_from(message.len()).expect("short")];
    record.extend_from_slice(&message);
    record
}

#[test]
fn framing() {
    let mut once = Some(IDENTITY);
    let first = block(&[1, 2, 3], CONTINUE, false, &mut once).expect("block");
    assert_eq!(&first[..16], &IDENTITY);
    assert_eq!(&first[16..19], &[CONTINUE, 0, 3]);
    let pad = usize::from(u16::from_be_bytes([first[19], first[20]]));
    assert!(pad < 256);
    assert_eq!(&first[21..24], &[1, 2, 3]);
    assert_eq!(first.len(), 24 + pad);
    assert!(first[24..].iter().all(|byte| *byte == 0));
    let second = block(&[4], END, true, &mut once).expect("block");
    assert_eq!(second[0], END);
    let long = usize::from(u16::from_be_bytes([second[3], second[4]]));
    assert!((899..1399).contains(&long), "{long}");
}

#[test]
fn unpadding() {
    let mut once = Some(IDENTITY);
    let mut wire = block(b"hello ", CONTINUE, true, &mut once).expect("block");
    wire.extend_from_slice(&block(b"world", END, false, &mut once).expect("block"));
    wire.extend_from_slice(b" tail");
    let mut traffic = Traffic::new(IDENTITY);
    let (left, right) = wire.split_at(30);
    let mut plain = unshape(&mut traffic, left);
    settle(&mut traffic);
    assert!(traffic.within);
    plain.extend_from_slice(&unshape(&mut traffic, right));
    settle(&mut traffic);
    assert_eq!(plain, b"hello world tail");
    assert!(!traffic.within && !traffic.direct);
    let stranger = unshape(&mut Traffic::new([1u8; 16]), &wire);
    assert_eq!(
        stranger, wire,
        "a stream that does not open with the identity is not padded"
    );
}

#[test]
fn direct() {
    let mut once = Some(IDENTITY);
    let wire = block(b"x", DIRECT, false, &mut once).expect("block");
    let mut traffic = Traffic::new(IDENTITY);
    assert_eq!(unshape(&mut traffic, &wire), b"x");
    settle(&mut traffic);
    assert!(traffic.direct);
}

#[test]
fn switching() {
    let mut traffic = Traffic::new(IDENTITY);
    let hello = [0x16, 3, 1, 0, 2, 1, 0];
    let opening = shape(&mut traffic, &hello).expect("shape");
    assert!(traffic.tls && !opening.switch);
    traffic.filter(&[&greeting()]);
    assert!(traffic.xtls && traffic.modern);
    assert_eq!(traffic.count, 0);
    let data = [0x17, 3, 3, 0, 2, 9, 9];
    let shaped = shape(&mut traffic, &data).expect("shape");
    assert!(shaped.switch);
    assert_eq!(shaped.blocks.len(), 1);
    assert_eq!(shaped.blocks[0][0], DIRECT);
    assert!(!traffic.padding);
    let after = shape(&mut traffic, b"raw").expect("shape");
    assert_eq!(after.blocks, vec![b"raw".to_vec()]);
}

#[test]
fn plaintext() {
    let mut traffic = Traffic::new(IDENTITY);
    let mut ended = false;
    for _ in 0..8 {
        let shaped = shape(&mut traffic, b"GET / HTTP/1.1\r\n").expect("shape");
        ended |= shaped
            .blocks
            .iter()
            .any(|item| item.get(16) == Some(&END) || item.first() == Some(&END));
        assert!(!shaped.switch);
    }
    assert!(
        ended && !traffic.padding,
        "padding ends for a stream that is not TLS"
    );
}

#[test]
fn records() {
    assert!(whole(&[0x17, 3, 3, 0, 1, 7, 0x17, 3, 3, 0, 2, 7, 7]));
    assert!(!whole(&[0x17, 3, 3, 0, 2, 7]));
    assert!(!whole(&[0x16, 3, 3, 0, 1, 7]));
    let body = vec![1u8; 9000];
    let pieces = reshape(&body);
    let sizes: Vec<usize> = pieces.iter().map(|piece| piece.len()).collect();
    assert_eq!(sizes, vec![4096, 4096, 808]);
}
