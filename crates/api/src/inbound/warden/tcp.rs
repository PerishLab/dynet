use super::{Admission, Call, Packet, Post, REST, Reply, frame};
use crate::inbound::divert::Budget;
use crate::inbound::lasting;
use crate::inbound::warren::Warren;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

pub(super) fn attend(warren: &Warren, span: Duration, socket: &TcpListener, admission: &Admission) {
    let started = Instant::now();
    let budget = Budget::new(32);
    std::thread::scope(|scope| {
        while lasting(started, span) && !warren.halted() {
            let Ok((stream, peer)) = socket.accept() else {
                std::thread::sleep(REST);
                continue;
            };
            let Some(permit) = budget.enter() else {
                continue;
            };
            scope.spawn(move || {
                let _permit = permit;
                let live = || lasting(started, span) && !warren.halted();
                let _ = serve((warren, admission), stream, peer, live);
            });
        }
    });
}

fn serve(
    context: (&Warren, &Admission),
    mut stream: TcpStream,
    peer: SocketAddr,
    live: impl Fn() -> bool,
) -> std::io::Result<()> {
    let (warren, admission) = context;
    stream.set_read_timeout(Some(REST))?;
    stream.set_write_timeout(Some(REST))?;
    stream.set_nodelay(true)?;
    while let Some(body) = frame::read(
        &mut stream,
        &live,
        (Duration::from_secs(30), Duration::from_secs(2)),
    )? {
        let Ok(call) = Call::new(&warren.current(), body, peer) else {
            break;
        };
        let post = Post {
            warren,
            reply: Reply::Stream(&stream),
        };
        let Some(_permit) = admission.enter(&call) else {
            let _ = post.speak(&Packet::new(&call.asked).failed(&call.query), &call);
            break;
        };
        if !post.answer(call) {
            break;
        }
    }
    Ok(())
}
