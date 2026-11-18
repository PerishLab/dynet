use super::{Frames, LIMIT, frame, request};

#[test]
fn wire() {
    assert_eq!(
        request(("192.0.2.1", 4500)).unwrap(),
        [1, 1, 192, 0, 2, 1, 17, 148]
    );
    assert!(request(("::1", 4500)).is_err());
    assert_eq!(frame(b"abc").unwrap(), b"\0\x03abc");
    assert!(frame(&vec![0; LIMIT + 1]).is_err());
}

#[test]
fn segments() {
    let mut chunks = [vec![0], vec![3, b'a'], vec![b'b', b'c', 0, 0, 0, 1, b'd']].into_iter();
    let mut frames = Frames::default();
    assert_eq!(
        frames.receive(|| Ok(chunks.next())).unwrap(),
        Some(b"abc".to_vec())
    );
    assert_eq!(
        frames.receive(|| Ok(chunks.next())).unwrap(),
        Some(Vec::new())
    );
    assert_eq!(
        frames.receive(|| Ok(chunks.next())).unwrap(),
        Some(b"d".to_vec())
    );
    assert_eq!(frames.receive(|| Ok(chunks.next())).unwrap(), None);
}

#[test]
fn malformed() {
    for body in [vec![0], vec![0, 2, 1], vec![255, 255]] {
        let mut chunks = Some(body);
        assert!(Frames::default().receive(|| Ok(chunks.take())).is_err());
    }
}
