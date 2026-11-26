mod trace;

use crate::outbound::{Circuit, Failure, Listener, Passage, Speaker};
use dynet_core::{Fault, Verdict};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread;
use std::time::Duration;
use trace::Trace;

pub const DEPTH: usize = 4;

struct Errand {
    passage: Passage,
    target: String,
    port: u16,
    bearing: bool,
    told: Sender<Outcome>,
}

struct Outcome {
    verdict: Verdict,
    failure: Option<Failure>,
}

pub enum Taken {
    Body(Vec<u8>),
    Empty,
    Spent,
}

pub struct Link {
    upward: Option<SyncSender<Vec<u8>>>,
    downward: Receiver<Vec<u8>>,
    told: Receiver<Outcome>,
    trace: Option<Trace>,
    circuit: Option<Arc<Circuit>>,
}

impl Link {
    pub fn open(passage: Passage, target: String, port: u16) -> Self {
        Self::start(passage, target, port, false)
    }

    pub fn bear(passage: Passage, target: String, port: u16) -> Self {
        Self::start(passage, target, port, true)
    }

    fn start(passage: Passage, target: String, port: u16, bearing: bool) -> Self {
        let passage = match passage.circuit() {
            Some(_) => passage,
            None => passage.gated(Arc::new(Circuit::new())),
        };
        let (upward, outgoing) = sync_channel(DEPTH);
        let (incoming, downward) = sync_channel(DEPTH);
        let (spoken, told) = channel();
        let circuit = passage.circuit();
        let trace = (!bearing && passage.chained()).then(Trace::new);
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
            circuit,
            trace,
        }
    }

    pub fn wait(&self) -> Option<Vec<u8>> {
        self.downward.recv().ok()
    }

    pub fn bide(&self, patience: Duration) -> Taken {
        if self.closed() {
            return Taken::Spent;
        }
        match self
            .downward
            .recv_timeout(patience.min(Duration::from_millis(200)))
        {
            Ok(body) => Taken::Body(body),
            Err(RecvTimeoutError::Timeout) => Taken::Empty,
            Err(RecvTimeoutError::Disconnected) => Taken::Spent,
        }
    }

    pub fn circuit(&self) -> Option<Arc<Circuit>> {
        self.circuit.clone()
    }

    pub fn closed(&self) -> bool {
        self.circuit
            .as_ref()
            .is_some_and(|circuit| circuit.closed())
    }

    pub fn allow(&self, action: impl FnOnce() -> bool) -> bool {
        match &self.circuit {
            Some(circuit) => circuit.allow(action).unwrap_or(false),
            None => action(),
        }
    }

    pub fn upward(&mut self) -> Option<SyncSender<Vec<u8>>> {
        self.upward.take()
    }

    pub(crate) fn request(&self) -> Option<&str> {
        self.trace.as_ref().map(|trace| trace.request.as_str())
    }

    pub fn verdict(&self) -> Option<Verdict> {
        self.told.try_recv().ok().map(|outcome| outcome.verdict)
    }

    pub(crate) fn report(&self, tell: &dyn Fn(&str)) -> Option<Verdict> {
        let result = self.told.try_recv().ok();
        if let Some(trace) = &self.trace {
            tell(
                &trace.line(
                    result
                        .as_ref()
                        .map(|outcome| (outcome.verdict, outcome.failure.as_ref())),
                ),
            );
        }
        result.map(|outcome| outcome.verdict)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(circuit) = &self.circuit {
            let _ = circuit.close();
        }
    }
}

fn carry(errand: Errand, outgoing: Receiver<Vec<u8>>, incoming: SyncSender<Vec<u8>>) {
    let (egress, ingress) = match reach(&errand) {
        Err(outcome) => {
            let _ = errand.told.send(outcome);
            return;
        }
        Ok(halves) => halves,
    };
    let writer = thread::spawn(move || push(egress, outgoing));
    let _ = errand.told.send(pull(ingress, &incoming));
    drop(incoming);
    let _ = writer.join();
}

fn reach(errand: &Errand) -> Result<(Speaker, Listener), Outcome> {
    let (target, port) = (&errand.target, errand.port);
    if errand.bearing {
        return errand.passage.bear(target, port).map_err(|fault| Outcome {
            verdict: Verdict::Faulted(fault),
            failure: None,
        });
    }
    errand
        .passage
        .observed(target, port)
        .map_err(|failure| Outcome {
            verdict: Verdict::Faulted(failure.fault),
            failure: Some(failure),
        })
}

fn push(mut egress: Speaker, outgoing: Receiver<Vec<u8>>) {
    while let Ok(body) = outgoing.recv() {
        if egress.send(&body).is_err() {
            break;
        }
    }
    let _ = egress.done();
}

fn pull(mut ingress: Listener, incoming: &SyncSender<Vec<u8>>) -> Outcome {
    let mut answered = false;
    let mut fault = Fault::Silent;
    let mut kind = None;
    loop {
        let taken = ingress.receive();
        match &taken {
            Err(error) => {
                fault = ingress.fault().unwrap_or(Fault::Severed);
                kind = error.kind();
                break;
            }
            Ok(None) => {
                kind = Some(std::io::ErrorKind::UnexpectedEof);
                break;
            }
            Ok(Some(_)) => answered = true,
        }
        if let Ok(Some(part)) = taken
            && incoming.send(part).is_err()
        {
            break;
        }
    }
    match answered {
        true => Outcome {
            verdict: Verdict::Answered,
            failure: None,
        },
        false => Outcome {
            verdict: Verdict::Faulted(fault),
            failure: Some(Failure {
                stage: ingress.stage(),
                fault,
                kind,
            }),
        },
    }
}
