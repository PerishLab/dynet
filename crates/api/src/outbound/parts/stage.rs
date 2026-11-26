use dynet_core::{Error, Fault};
use std::io::ErrorKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stage {
    Connect,
    Handshake,
    Request,
    Open,
    Relay,
}

pub(crate) struct Failure {
    pub(crate) stage: Stage,
    pub(crate) fault: Fault,
    pub(crate) kind: Option<ErrorKind>,
}

impl Stage {
    pub(crate) fn failed(self, fault: Fault, error: Error) -> Failure {
        Failure {
            stage: self,
            fault,
            kind: error.kind(),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Connect => "entry_connect",
            Self::Handshake => "entry_handshake",
            Self::Request | Self::Open => "next_hop_open",
            Self::Relay => "relay",
        }
    }

    pub(crate) fn last(self) -> &'static str {
        match self {
            Self::Connect => "none",
            Self::Handshake => "entry_connected",
            Self::Request => "entry_request_sent",
            Self::Open => "entry_handshake",
            Self::Relay => "next_hop_open",
        }
    }

    pub(crate) fn hop(self) -> &'static str {
        match self {
            Self::Connect | Self::Handshake => "entry",
            Self::Request | Self::Open | Self::Relay => "exit",
        }
    }
}
