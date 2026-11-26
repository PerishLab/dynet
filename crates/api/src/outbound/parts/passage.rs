use super::{Circuit, Egress, Exit, Failure, Front, Ingress, Listener, Speaker, Stage, uot};
use crate::outbound::{veil, vmess::Tunnel};
use dynet_core::Fault;
use std::sync::Arc;
use std::time::Duration;
const PATIENCE: Duration = Duration::from_secs(5);

pub enum Passage {
    Plain(Front),
    Veiled(Front, Exit),
    Direct(u32),
    Guarded(Box<Passage>, Arc<Circuit>),
}

impl Passage {
    pub fn open(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        self.observed(host, port).map_err(|failure| failure.fault)
    }

    pub(crate) fn observed(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Failure> {
        self.going(host, port, None)
    }

    pub(crate) fn chained(&self) -> bool {
        match self {
            Self::Guarded(passage, _) => passage.chained(),
            Self::Veiled(..) => true,
            _ => false,
        }
    }

    fn going(
        &self,
        host: &str,
        port: u16,
        gate: Option<&Circuit>,
    ) -> Result<(Speaker, Listener), Failure> {
        match self {
            Self::Guarded(passage, circuit) => passage.going(host, port, Some(circuit)),
            Self::Plain(front) => {
                let (egress, ingress) = front.boarding(host, port, gate)?;
                Ok((Speaker::Plain(egress), Listener::Plain(ingress)))
            }
            Self::Veiled(front, exit) => veiled(front, exit, (host, port), gate),
            Self::Direct(mark) => direct(host, port, *mark, gate),
        }
    }

    pub fn bearing(&self) -> bool {
        match self {
            Self::Guarded(passage, _) => passage.bearing(),
            Self::Plain(front) => front.bearing(),
            Self::Veiled(_, exit) => exit.relay,
            _ => false,
        }
    }

    pub fn bear(&self, host: &str, port: u16) -> Result<(Speaker, Listener), Fault> {
        self.carrying(host, port, None)
    }

    fn carrying(
        &self,
        host: &str,
        port: u16,
        gate: Option<&Circuit>,
    ) -> Result<(Speaker, Listener), Fault> {
        if let Self::Guarded(passage, circuit) = self {
            return passage.carrying(host, port, Some(circuit));
        }
        if let Self::Veiled(front, exit) = self {
            return relayed(front, exit, (host, port), gate);
        }
        let Self::Plain(Front::Vmess(endpoint)) = self else {
            return Err(Fault::Handshake);
        };
        let mut tunnel = Tunnel::connect(endpoint, gate).map_err(|_| Fault::Reach)?;
        tunnel
            .bear(endpoint, host, port)
            .map_err(|_| Fault::Handshake)?;
        let (egress, ingress) = tunnel.split();
        Ok((
            Speaker::Plain(Egress::Vmess(Box::new(egress))),
            Listener::Plain(Ingress::Vmess(Box::new(ingress))),
        ))
    }
    pub fn gated(self, circuit: Arc<Circuit>) -> Self {
        Self::Guarded(Box::new(self), circuit)
    }

    pub fn circuit(&self) -> Option<Arc<Circuit>> {
        match self {
            Self::Guarded(_, circuit) => Some(Arc::clone(circuit)),
            _ => None,
        }
    }
}

fn direct(
    host: &str,
    port: u16,
    mark: u32,
    gate: Option<&Circuit>,
) -> Result<(Speaker, Listener), Failure> {
    let failed = |error| Stage::Connect.failed(Fault::Reach, error);
    let seat = crate::outbound::bond::place(host, port).map_err(failed)?;
    let stream = crate::outbound::bond::reach(seat, mark, PATIENCE).map_err(failed)?;
    let stream = if let Some(gate) = gate {
        gate.attach(stream).map_err(failed)?
    } else {
        stream
    };
    let again = stream.try_clone().map_err(|error| {
        failed(dynet_core::Error::io(
            "cannot halve the direct session",
            error,
        ))
    })?;
    Ok((
        Speaker::Plain(Egress::Raw(stream)),
        Listener::Plain(Ingress::Raw(again)),
    ))
}

fn veiled(
    front: &Front,
    exit: &Exit,
    seat: (&str, u16),
    gate: Option<&Circuit>,
) -> Result<(Speaker, Listener), Failure> {
    let (mut egress, ingress) = front.boarding(&exit.host, exit.port, gate)?;
    let stage = match ingress.opening() {
        Stage::Handshake => Stage::Request,
        stage => stage,
    };
    let failed = |error| stage.failed(Fault::Handshake, error);
    let (mut cloak, shroud) = veil(exit.secret).map_err(failed)?;
    cloak.greet(&mut egress, seat, &[]).map_err(failed)?;
    Ok((
        Speaker::Veiled(egress, Box::new(cloak)),
        Listener::Veiled(ingress, Box::new(shroud)),
    ))
}

fn relayed(
    front: &Front,
    exit: &Exit,
    seat: (&str, u16),
    gate: Option<&Circuit>,
) -> Result<(Speaker, Listener), Fault> {
    if !exit.relay {
        return Err(Fault::Handshake);
    }
    let request = uot::request(seat).map_err(|_| Fault::Handshake)?;
    let (mut egress, ingress) = front
        .boarding(&exit.host, exit.port, gate)
        .map_err(|failure| failure.fault)?;
    let (mut cloak, shroud) = veil(exit.secret).map_err(|_| Fault::Handshake)?;
    cloak
        .greet(&mut egress, (uot::HOST, 0), &request)
        .map_err(|_| Fault::Handshake)?;
    Ok((
        Speaker::Relayed(egress, Box::new(cloak)),
        Listener::Relayed(ingress, Box::new(shroud), uot::Frames::default()),
    ))
}
