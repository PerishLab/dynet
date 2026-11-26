use crate::outbound::{Failure, Stage};
use dynet_core::Verdict;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub(super) struct Trace {
    pub(super) request: String,
    since: Instant,
}

impl Trace {
    pub(super) fn new() -> Self {
        Self {
            request: format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
            since: Instant::now(),
        }
    }

    pub(super) fn line(&self, result: Option<(Verdict, Option<&Failure>)>) -> String {
        let prefix = format!(
            "request={} attempt=1 elapsed_ms={}",
            self.request,
            self.since.elapsed().as_millis()
        );
        match result {
            Some((verdict, Some(failure))) => format!(
                "{prefix} result={verdict:?} hop={} stage={} last={} error={}",
                failure.stage.hop(),
                failure.stage.name(),
                failure.stage.last(),
                kind(failure.kind)
            ),
            Some((verdict, None)) => {
                format!("{prefix} result={verdict:?} last={}", Stage::Relay.last())
            }
            None => format!("{prefix} result=cancelled"),
        }
    }
}

fn kind(kind: Option<ErrorKind>) -> String {
    match kind {
        Some(ErrorKind::TimedOut | ErrorKind::WouldBlock) => "timeout".into(),
        Some(kind) => format!("{kind:?}"),
        None => "operation_failed".into(),
    }
}
