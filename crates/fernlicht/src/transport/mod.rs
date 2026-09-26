//! Getting UDS requests to a module and the replies back.

mod framing;

pub use framing::{Framing, Incoming, doip_identification_request};
