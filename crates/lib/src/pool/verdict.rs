#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    Reach,
    Handshake,
    Refused,
    Silent,
    Severed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blame {
    Node,
    Target,
    Unclear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Answered,
    Blocked,
    Faulted(Fault),
}

impl Fault {
    pub fn blame(self) -> Blame {
        match self {
            Self::Reach | Self::Handshake => Blame::Node,
            Self::Refused => Blame::Target,
            Self::Silent | Self::Severed => Blame::Unclear,
        }
    }
}

impl Blame {
    pub fn charges(self) -> bool {
        matches!(self, Self::Node | Self::Unclear)
    }
}

impl Verdict {
    pub fn working(self) -> bool {
        match self {
            Self::Answered | Self::Blocked => true,
            Self::Faulted(fault) => !fault.blame().charges(),
        }
    }

    pub fn blame(self) -> Option<Blame> {
        match self {
            Self::Answered | Self::Blocked => None,
            Self::Faulted(fault) => Some(fault.blame()),
        }
    }
}
