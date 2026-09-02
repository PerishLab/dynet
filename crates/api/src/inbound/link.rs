use crate::outbound::{Egress, Endpoint, Ingress, Tunnel};
use dynet_core::{Fault, Verdict};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

struct Errand {
    endpoint: Endpoint,
    target: String,
    port: u16,
    told: Sender<Verdict>,
}

pub enum Taken {
    Body(Vec<u8>),
    Empty,
    Spent,
}

pub struct Link {
    upward: Option<Sender<Vec<u8>>>,
    downward: Receiver<Vec<u8>>,
    told: Receiver<Verdict>,
}

impl Link {
    pub fn open(endpoint: Endpoint, target: String, port: u16) -> Self {
        let (upward, outgoing) = channel();
        let (incoming, downward) = channel();
        let (spoken, told) = channel();
        let errand = Errand {
            endpoint,
            target,
            port,
            told: spoken,
        };
        thread::spawn(move || carry(errand, outgoing, incoming));
        Self {
            upward: Some(upward),
            downward,
            told,
        }
    }

    pub fn offer(&mut self, body: &[u8]) -> bool {
        let Some(upward) = self.upward.as_ref() else {
            return false;
        };
        upward.send(body.to_vec()).is_ok()
    }

    pub fn take(&mut self) -> Taken {
        match self.downward.try_recv() {
            Ok(body) => Taken::Body(body),
            Err(TryRecvError::Empty) => Taken::Empty,
            Err(TryRecvError::Disconnected) => Taken::Spent,
        }
    }

    pub fn hush(&mut self) {
        self.upward = None;
    }

    pub fn verdict(&self) -> Option<Verdict> {
        self.told.try_recv().ok()
    }
}

fn carry(errand: Errand, outgoing: Receiver<Vec<u8>>, incoming: Sender<Vec<u8>>) {
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

fn reach(errand: &Errand) -> Result<(Egress, Ingress), Fault> {
    let mut tunnel = Tunnel::dial(&errand.endpoint).map_err(|_| Fault::Reach)?;
    tunnel
        .board(&errand.endpoint, &errand.target, errand.port)
        .map_err(|_| Fault::Handshake)?;
    Ok(tunnel.split())
}

fn push(mut egress: Egress, outgoing: Receiver<Vec<u8>>) {
    while let Ok(body) = outgoing.recv() {
        if egress.send(&body).is_err() {
            break;
        }
    }
    let _ = egress.done();
}

fn pull(mut ingress: Ingress, incoming: &Sender<Vec<u8>>) -> Verdict {
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
