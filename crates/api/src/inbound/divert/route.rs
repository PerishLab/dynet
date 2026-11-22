use super::Divert;
use super::noise::Key;
use crate::inbound::warren::Chosen;
use dynet_core::{Bearing, Course, Decision, Ground, Refusal, Sight};
use std::net::{IpAddr, SocketAddrV4};

impl Divert<'_> {
    pub(in crate::inbound) fn route(
        &self,
        target: SocketAddrV4,
        sight: &Sight,
    ) -> Option<(Chosen, String)> {
        let (decision, selection) = self.warren.decide(sight)?;
        let generation = selection.generation();
        if decision.ground() == Ground::Named {
            self.tally(|served| served.named += 1);
        }
        let aimed = decision
            .aim()
            .map_or_else(|| target.ip().to_string(), |aim| aim.get().to_string());
        let chosen = match decision.course() {
            Course::Through(cluster) => {
                if let Some(aim) = decision.aim() {
                    (self.told())(&format!("{target} aim={}", aim.get()));
                }
                let chosen = self.warren.pick(selection, cluster, sight.bearing);
                if chosen.is_none() {
                    (self.told())(&format!(
                        "{target} found no node in cluster {}",
                        cluster.get()
                    ));
                }
                chosen
            }
            Course::Direct if sight.bearing == Bearing::Stream => {
                (self.told())(&format!(
                    "{target} leaves directly on {:?}",
                    decision.ground()
                ));
                self.warren.direct(selection)
            }
            Course::Direct | Course::Refused(..) => {
                self.refused(target, &decision, sight.bearing);
                None
            }
        };
        crate::inbound::evidence::route(
            self.reporter,
            crate::inbound::evidence::Selection {
                target,
                sight,
                decision: &decision,
                chosen: chosen.as_ref(),
                scope: "flow",
                generation,
            },
        );
        chosen.map(|chosen| (chosen, aimed))
    }

    pub(super) fn refused(&self, target: SocketAddrV4, decision: &Decision, bearing: Bearing) {
        match decision.course() {
            Course::Direct => {
                let line = || {
                    format!(
                        "{target} would leave directly, and datagrams are never sent from here; refused"
                    )
                };
                if target.ip().is_multicast() {
                    self.noise
                        .refuse(Key::Multicast(target.port()), line, self.told());
                } else {
                    (self.told())(&line());
                }
            }
            Course::Refused(cluster, refusal) => {
                let line = || {
                    format!(
                        "{target} refused on {:?}: cluster {} is {refusal:?}",
                        decision.ground(),
                        cluster.get()
                    )
                };
                if bearing == Bearing::Datagram && matches!(refusal, Refusal::Unborne) {
                    self.noise.refuse(
                        Key::Unborne(cluster.clone(), target.port()),
                        line,
                        self.told(),
                    );
                } else {
                    (self.told())(&line());
                }
            }
            Course::Through(_) => {}
        }
    }

    pub(in crate::inbound) fn issued(&self, target: SocketAddrV4) -> Option<String> {
        let ledger = self.warren.ledger.lock().ok()?;
        ledger
            .lookup(IpAddr::V4(*target.ip()))
            .map(|issue| issue.domain().get().to_string())
    }
}
