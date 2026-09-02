use super::link::{Link, Taken};
use smoltcp::socket::tcp;

pub struct Strand {
    pub link: Link,
    pub pending: Vec<u8>,
    pub hushed: bool,
    pub faulted: bool,
}

impl Strand {
    pub fn new(link: Link) -> Self {
        Self {
            link,
            pending: Vec::new(),
            hushed: false,
            faulted: false,
        }
    }
}

pub fn upward(socket: &mut tcp::Socket, strand: &mut Strand) {
    if socket.can_recv() {
        let taken = socket
            .recv(|data| (data.len(), data.to_vec()))
            .unwrap_or_default();
        if !taken.is_empty() {
            strand.link.offer(&taken);
        }
    }
    if !socket.may_recv() && !strand.hushed {
        strand.link.hush();
        strand.hushed = true;
    }
}

fn finish(socket: &mut tcp::Socket, faulted: bool) {
    match faulted {
        true => socket.abort(),
        false => socket.close(),
    }
}

pub fn downward(socket: &mut tcp::Socket, strand: &mut Strand) {
    if strand.pending.is_empty() {
        match strand.link.take() {
            Taken::Body(body) => strand.pending = body,
            Taken::Empty => return,
            Taken::Spent => return finish(socket, strand.faulted),
        }
    }
    if !socket.can_send() {
        return;
    }
    let written = socket.send_slice(&strand.pending).unwrap_or_default();
    strand.pending.drain(..written);
}
