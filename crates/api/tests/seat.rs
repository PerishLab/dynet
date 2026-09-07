use dynet_api::host::Veil;
use std::net::{Ipv4Addr, UdpSocket};

const LOOP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

fn spare() -> u16 {
    UdpSocket::bind((LOOP, 0))
        .expect("a spare port")
        .local_addr()
        .expect("its number")
        .port()
}

#[test]
fn answers() {
    let port = spare();
    let held = UdpSocket::bind((LOOP, port)).expect("a seat to hold");
    assert!(
        Veil::attended(LOOP, port),
        "a seat something is holding reads as answering"
    );
    drop(held);
    assert!(
        !Veil::attended(LOOP, port),
        "a seat nobody holds reads as silent, which is what a stranded rule set looks like"
    );
}

#[test]
fn absent() {
    assert!(
        !Veil::attended(Ipv4Addr::new(203, 0, 113, 7), spare()),
        "an address this host does not wear is silent rather than answering, because a bind that fails for want of an address is not evidence of a listener"
    );
}
