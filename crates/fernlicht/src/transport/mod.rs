//! Getting UDS requests to a module and the replies back.
//!
//! [`Framing`] knows the two wire formats, [`Client`] runs requests over any
//! [`Transport`], and [`connect`] opens a TCP connection to the gateway.

mod client;
mod framing;
mod tcp;

use std::io;
use std::time::Duration;

pub use client::{Client, ClientOptions, Direction, Tracer};
pub use framing::{Framing, Incoming, doip_identification_request};
pub use tcp::{DEFAULT_ORDER, connect, connect_port, discover};

use crate::uds::Reply;
use crate::{Error, Result};

/// Default timeout of [`UdsLink::send`].
pub const SEND_TIMEOUT: Duration = Duration::from_millis(900);

/// A byte stream to the gateway. Implemented for [`std::net::TcpStream`];
/// implement it to run the client over anything else.
pub trait Transport: Send {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Reads whatever has arrived, waiting at most `timeout`.
    ///
    /// `Ok(0)` means the peer closed the connection. An error of kind
    /// [`WouldBlock`](io::ErrorKind::WouldBlock) or
    /// [`TimedOut`](io::ErrorKind::TimedOut) means nothing arrived in time.
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
}

/// Anything that can carry a UDS request to a module. Drivers and the
/// identification code depend on this rather than on [`Client`].
pub trait UdsLink {
    fn request(&self, ecu: u16, request: &[u8], timeout: Duration) -> Result<Reply>;

    /// Sends a request and turns a negative reply into [`Error::Negative`].
    /// Returns the data of the positive reply.
    fn send(&self, ecu: u16, request: &[u8]) -> Result<Vec<u8>> {
        match self.request(ecu, request, SEND_TIMEOUT)? {
            Reply::Positive { data, .. } => Ok(data),
            Reply::Negative { nrc, .. } => Err(Error::Negative { ecu, nrc }),
        }
    }
}

impl<L: UdsLink + ?Sized> UdsLink for &L {
    fn request(&self, ecu: u16, request: &[u8], timeout: Duration) -> Result<Reply> {
        (**self).request(ecu, request, timeout)
    }
}
