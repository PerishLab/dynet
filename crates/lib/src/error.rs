use std::{fmt, io};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(String, Option<io::ErrorKind>);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into(), None)
    }

    pub fn io(context: &str, error: io::Error) -> Self {
        let kind = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<Self>())
            .and_then(Self::kind)
            .unwrap_or_else(|| error.kind());
        Self(format!("{context}: {error}"), Some(kind))
    }

    pub fn kind(&self) -> Option<io::ErrorKind> {
        self.1
    }

    pub fn wrapped(self) -> io::Error {
        io::Error::other(self)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
