#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Bearing {
    Stream,
    Datagram,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Carriage {
    Native,
    Associate,
    Relay,
    Absent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability {
    carriage: Carriage,
}

impl Carriage {
    pub fn datagrams(self) -> bool {
        !matches!(self, Self::Absent)
    }
}

impl Capability {
    pub fn new(carriage: Carriage) -> Self {
        Self { carriage }
    }

    pub fn carriage(self) -> Carriage {
        self.carriage
    }

    pub fn datagrams(self) -> bool {
        self.carriage.datagrams()
    }
}

impl Capability {
    pub fn bears(self, bearing: Bearing) -> bool {
        match bearing {
            Bearing::Stream => true,
            Bearing::Datagram => self.datagrams(),
        }
    }
}
