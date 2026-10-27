use super::Divert;
use crate::inbound::warren::Chosen;
use dynet_core::{Bearing, Course, Ground, Sight};
use std::net::{IpAddr, SocketAddrV4};

impl Divert<'_> {
    pub(in crate::inbound) fn route(
        &self,
        target: SocketAddrV4,
        sight: &Sight,
    ) -> Option<(Chosen, String)> {
        let (decision, selection) = self.warren.decide(sight)?;
        if decision.ground() == Ground::Named {
            self.tally(|served| served.named += 1);
        }
        let aimed = decision
            .aim()
            .map_or_else(|| target.ip().to_string(), |aim| aim.get().to_string());
        match decision.course() {
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
                Some((chosen?, aimed))
            }
            Course::Direct if sight.bearing == Bearing::Stream => {
                (self.told())(&format!(
                    "{target} leaves directly on {:?}",
                    decision.ground()
                ));
                Some((self.warren.direct(selection)?, aimed))
            }
            Course::Direct => {
                (self.told())(&format!(
                    "{target} would leave directly, and datagrams are never sent from here; refused"
                ));
                None
            }
            Course::Refused(cluster, refusal) => {
                (self.told())(&format!(
                    "{target} refused on {:?}: cluster {} is {refusal:?}",
                    decision.ground(),
                    cluster.get()
                ));
                None
            }
        }
    }

    pub(in crate::inbound) fn issued(&self, target: SocketAddrV4) -> Option<String> {
        let ledger = self.warren.ledger.lock().ok()?;
        ledger
            .lookup(IpAddr::V4(*target.ip()))
            .map(|issue| issue.domain().get().to_string())
    }
}
