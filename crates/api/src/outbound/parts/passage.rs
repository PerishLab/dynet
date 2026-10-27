use super::{Circuit, Egress, Exit, Front, Ingress, Listener, Speaker};
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
        self.going(host, port, None)
    }

    fn going(
        &self,
        host: &str,
        port: u16,
        gate: Option<&Circuit>,
    ) -> Result<(Speaker, Listener), Fault> {
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
) -> Result<(Speaker, Listener), Fault> {
    let seat = crate::outbound::bond::place(host, port).map_err(|_| Fault::Reach)?;
    let stream = crate::outbound::bond::reach(seat, mark, PATIENCE).map_err(|_| Fault::Reach)?;
    let stream = if let Some(gate) = gate {
        gate.attach(stream).map_err(|_| Fault::Reach)?
    } else {
        stream
    };
    let again = stream.try_clone().map_err(|_| Fault::Reach)?;
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
) -> Result<(Speaker, Listener), Fault> {
    let (mut egress, ingress) = front.boarding(&exit.host, exit.port, gate)?;
    let (mut cloak, shroud) = veil(exit.secret).map_err(|_| Fault::Handshake)?;
    cloak
        .greet(&mut egress, seat, &[])
        .map_err(|_| Fault::Handshake)?;
    Ok((
        Speaker::Veiled(egress, Box::new(cloak)),
        Listener::Veiled(ingress, Box::new(shroud)),
    ))
}
