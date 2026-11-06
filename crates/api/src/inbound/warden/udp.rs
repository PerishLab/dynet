use super::super::lasting;
use super::super::warren::Warren;
use super::{Admission, Call, Packet, Post, ROOM, Reply};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

pub(super) fn attend(warren: &Warren, span: Duration, socket: &UdpSocket, admission: &Admission) {
    let post = Post {
        warren,
        reply: Reply::Datagram(socket),
    };
    let started = Instant::now();
    std::thread::scope(|scope| {
        while lasting(started, span) && !warren.halted() {
            let mut room = [0u8; ROOM];
            let Ok((size, peer)) = socket.recv_from(&mut room) else {
                continue;
            };
            let held = warren.current();
            let Ok(call) = Call::new(&held, room[..size].to_vec(), peer) else {
                continue;
            };
            let Some(permit) = admission.enter(&call) else {
                let reply = Packet::new(&call.asked).failed(&call.query);
                let _ = post.speak(&reply, &call);
                continue;
            };
            let post = &post;
            scope.spawn(move || {
                let _permit = permit;
                post.answer(call);
            });
        }
    });
}
