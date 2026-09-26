use std::{fmt, io};

use crate::uds::Nrc;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The socket failed.
    Io(io::Error),
    /// The DoIP gateway did not answer the routing activation in time.
    ActivationTimeout,
    /// The connection was closed, by the caller or after a fatal error.
    Closed,
    /// The gateway rejected a message: unknown address, NACK, refused activation.
    Refused(String),
    /// Bytes on the wire that do not form a valid frame.
    Protocol(String),
    /// The request can not be sent as given.
    InvalidRequest(&'static str),
    /// The module did not answer in time.
    Timeout {
        ecu: u16,
    },
    /// This request timed out earlier on the same connection. A late reply to
    /// the first attempt would be indistinguishable from the answer to a retry.
    Stale {
        ecu: u16,
    },
    /// The module answered with a negative response.
    Negative {
        ecu: u16,
        nrc: Nrc,
    },
    InvalidHex(String),
}

impl Error {
    /// Errors after which the connection can not be used any more.
    pub(crate) fn is_fatal(&self) -> bool {
        matches!(self, Error::Io(_) | Error::Closed | Error::Refused(_) | Error::Protocol(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::ActivationTimeout => f.write_str("gateway did not activate routing in time"),
            Error::Closed => f.write_str("connection closed"),
            Error::Refused(reason) | Error::Protocol(reason) => f.write_str(reason),
            Error::InvalidRequest(reason) => write!(f, "invalid request: {reason}"),
            Error::Timeout { ecu } => write!(f, "timeout waiting for ecu {ecu:#04x}"),
            Error::Stale { ecu } => {
                write!(f, "ecu {ecu:#04x} timed out on this request earlier, reconnect before retrying")
            }
            Error::Negative { ecu, nrc } => write!(f, "ecu {ecu:#04x}: {nrc}"),
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
