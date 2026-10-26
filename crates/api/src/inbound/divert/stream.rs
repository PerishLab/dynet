use super::Divert;
use crate::inbound::IDLE;
use crate::inbound::link::{Link, Taken};
use crate::inbound::warren::Charge;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

const REST: Duration = Duration::from_millis(200);

impl Divert<'_> {
    pub(in crate::inbound) fn carry(
        &self,
        near: TcpStream,
        mut link: Link,
        charge: Option<&Charge>,
    ) {
        let Ok(again) = near.try_clone() else { return };
        let _ = again.set_read_timeout(Some(REST));
        let _ = near.set_write_timeout(Some(IDLE));
        let sending = link.upward();
        std::thread::scope(|scope| {
            scope.spawn(|| self.push(again, sending));
            self.drain(&near, &link);
            if self.spent() {
                let _ = near.shutdown(Shutdown::Both);
            }
        });
        self.judge(&link, charge);
    }

    fn push(&self, mut from: TcpStream, sending: Option<SyncSender<Vec<u8>>>) {
        let Some(sending) = sending else { return };
        let mut room = [0u8; 16384];
        while !self.spent() {
            let size = match from.read(&mut room) {
                Ok(0) => break,
                Ok(size) => size,
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    continue;
                }
                Err(_) => break,
            };
            if !self.send(&sending, room[..size].to_vec()) {
                break;
            }
        }
    }

    fn send(&self, sending: &SyncSender<Vec<u8>>, mut body: Vec<u8>) -> bool {
        while !self.spent() {
            match sending.try_send(body) {
                Ok(()) => return true,
                Err(TrySendError::Full(held)) => body = held,
                Err(TrySendError::Disconnected(_)) => return false,
            }
            std::thread::sleep(REST);
        }
        false
    }

    fn drain(&self, mut into: &TcpStream, link: &Link) {
        while !self.spent() {
            match link.bide(REST) {
                Taken::Body(body) if into.write_all(&body).is_err() => break,
                Taken::Spent => break,
                _ => {}
            }
        }
        let _ = into.shutdown(Shutdown::Write);
    }
}
