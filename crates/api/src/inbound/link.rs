use crate::outbound::{Listener, Passage, Speaker};
use dynet_core::{Fault, Verdict};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread;
use std::time::Duration;

pub const DEPTH: usize = 4;

struct Errand {
    passage: Passage,
    target: String,
    port: u16,
    bearing: bool,
    told: Sender<Verdict>,
}

pub enum Taken {
    Body(Vec<u8>),
    Empty,
    Spent,
}

pub struct Link {
    upward: Option<SyncSender<Vec<u8>>>,
    downward: Receiver<Vec<u8>>,
    told: Receiver<Verdict>,
}

impl Link {
    pub fn open(passage: Passage, target: String, port: u16) -> Self {
        Self::start(passage, target, port, false)
    }

    pub fn bear(passage: Passage, target: String, port: u16) -> Self {
        Self::start(passage, target, port, true)
    }

    fn start(passage: Passage, target: String, port: u16, bearing: bool) -> Self {
        let (upward, outgoing) = sync_channel(DEPTH);
        let (incoming, downward) = sync_channel(DEPTH);
        let (spoken, told) = channel();
        let errand = Errand {
            passage,
            target,
            port,
            bearing,
            told: spoken,
        };
        thread::spawn(move || carry(errand, outgoing, incoming));
        Self {
            upward: Some(upward),
            downward,
            told,
        }
    }

    pub fn wait(&self) -> Option<Vec<u8>> {
        self.downward.recv().ok()
    }

    pub fn bide(&self, patience: Duration) -> Taken {
        match self.downward.recv_timeout(patience) {
            Ok(body) => Taken::Body(body),
            Err(RecvTimeoutError::Timeout) => Taken::Empty,
            Err(RecvTimeoutError::Disconnected) => Taken::Spent,
        }
    }

    pub fn upward(&mut self) -> Option<SyncSender<Vec<u8>>> {
        self.upward.take()
    }

    pub fn verdict(&self) -> Option<Verdict> {
        self.told.try_recv().ok()
    }
}

fn carry(errand: Errand, outgoing: Receiver<Vec<u8>>, incoming: SyncSender<Vec<u8>>) {
    let (egress, ingress) = match reach(&errand) {
        Err(fault) => {
            let _ = errand.told.send(Verdict::Faulted(fault));
            return;
        }
        Ok(halves) => halves,
    };
    let writer = thread::spawn(move || push(egress, outgoing));
    let verdict = pull(ingress, &incoming);
    let _ = errand.told.send(verdict);
    drop(incoming);
    let _ = writer.join();
}

fn reach(errand: &Errand) -> Result<(Speaker, Listener), Fault> {
    match errand.bearing {
        true => errand.passage.bear(&errand.target, errand.port),
        false => errand.passage.open(&errand.target, errand.port),
    }
}

fn push(mut egress: Speaker, outgoing: Receiver<Vec<u8>>) {
    while let Ok(body) = outgoing.recv() {
        if egress.send(&body).is_err() {
            break;
        }
    }
    let _ = egress.done();
}

fn pull(mut ingress: Listener, incoming: &SyncSender<Vec<u8>>) -> Verdict {
    let mut answered = false;
    let mut fault = Fault::Silent;
    loop {
        let taken = ingress.receive();
        if taken.is_err() {
            fault = ingress.fault().unwrap_or(Fault::Severed);
            break;
        }
        let Ok(Some(part)) = taken else { break };
        answered = true;
        if incoming.send(part).is_err() {
            break;
        }
    }
    match answered {
        true => Verdict::Answered,
        false => Verdict::Faulted(fault),
    }
}
