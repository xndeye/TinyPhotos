use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Network,
    Authorization,
    UnsupportedDevice,
    RateLimited,
    Cancelled,
    UncertainCommit,
    UncertainAlbumCreation,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn stops_run(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Authorization | ErrorKind::UnsupportedDevice | ErrorKind::RateLimited
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::Other, message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
