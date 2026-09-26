use std::{fmt, io};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The socket failed.
    Io(io::Error),
    /// The gateway rejected a message: unknown address, NACK, refused activation.
    Refused(String),
    /// Bytes on the wire that do not form a valid frame.
    Protocol(String),
    InvalidHex(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Refused(reason) | Error::Protocol(reason) => f.write_str(reason),
            Error::InvalidHex(input) => write!(f, "not hex: {input:?}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}
