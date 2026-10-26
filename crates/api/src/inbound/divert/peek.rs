use super::Divert;
use crate::inbound::sniff::{self, Refused, Sniffed};
use crate::inbound::warren::Chosen;
use dynet_core::{Bearing, Sight, Told};
use std::net::{IpAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

const PEEK: usize = 16384;
const BUDGET: Duration = Duration::from_millis(300);
const PATIENCE: usize = 5;
const REST: Duration = Duration::from_millis(5);

impl Divert<'_> {
    pub(in crate::inbound) fn elect(
        &self,
        stream: &TcpStream,
        target: SocketAddrV4,
    ) -> Option<(Chosen, String)> {
        let sniffed = self.sniffed(stream, target);
        if let Some(told) = &sniffed {
            self.recite(target, told);
        }
        let sight = Sight {
            told: sniffed.and_then(|told| Told::read(&told.name, told.outer)),
            address: IpAddr::V4(*target.ip()),
            bearing: Bearing::Stream,
        };
        self.route(target, &sight)
    }

    fn recite(&self, target: SocketAddrV4, told: &Sniffed) {
        let issued = self.issued(target);
        let issued = issued.as_deref();
        let borrowed = match told.outer {
            true => ", a provider's public name and not the destination's own",
            false => "",
        };
        let Some(issued) = issued.filter(|held| *held != told.name) else {
            (self.told())(&format!("{target} names {}{borrowed}", told.name));
            return;
        };
        (self.told())(&format!(
            "{target} names {}{borrowed}, though its address was issued for {issued}",
            told.name
        ));
    }

    fn sniffed(&self, stream: &TcpStream, target: SocketAddrV4) -> Option<Sniffed> {
        if !sniff::worth(target.port()) || self.bashful(target) {
            return None;
        }
        let told = self.peek(stream);
        let _ = stream.set_read_timeout(None);
        self.noted(target, told.is_some());
        told
    }

    fn peek(&self, stream: &TcpStream) -> Option<Sniffed> {
        let started = Instant::now();
        let mut room = vec![0u8; PEEK];
        loop {
            let left = BUDGET
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())?;
            stream.set_read_timeout(Some(left)).ok()?;
            let size = stream.peek(&mut room).ok()?;
            match sniff::read(room.get(..size)?) {
                Ok(told) => return Some(told),
                Err(Refused::Need(more)) if more <= PEEK && more > size => {}
                Err(_) => return None,
            }
            std::thread::sleep(REST);
        }
    }

    fn noted(&self, target: SocketAddrV4, found: bool) {
        let Ok(mut shy) = self.shy.lock() else {
            return;
        };
        if found {
            shy.remove(&target);
            return;
        }
        *shy.entry(target).or_default() += 1;
    }

    fn bashful(&self, target: SocketAddrV4) -> bool {
        self.shy
            .lock()
            .is_ok_and(|shy| shy.get(&target).is_some_and(|spent| *spent >= PATIENCE))
    }
}
