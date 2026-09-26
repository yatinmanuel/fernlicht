use std::fmt;

use crate::bytes::{Hex, be_u16, be_u32};
use crate::{Error, Result};

/// Largest payload accepted from the gateway. Anything bigger means the
/// stream is out of sync.
const MAX_PAYLOAD: u32 = 1 << 20;

/// The two ways a BMW gateway frames diagnostic messages over TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Framing {
    /// BMW's HSFZ on port 6801. F-series.
    Hsfz,
    /// ISO 13400-2 DoIP on port 13400. G-series and later.
    Doip,
}

/// A decoded message from the gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Uds {
        src: u16,
        dst: u16,
        uds: Vec<u8>,
    },
    /// DoIP routing activation accepted.
    Activated,
    /// The gateway asks whether we are still there.
    AliveCheck,
    /// The gateway rejected something we sent.
    Refused(String),
    /// Acknowledgements and other traffic that needs no action.
    Ignored,
}

mod hsfz {
    pub const HEADER: usize = 6;
    pub const DIAGNOSTIC: u16 = 0x0001;

    pub fn error(kind: u16) -> Option<&'static str> {
        Some(match kind {
            0x40 => "incorrect tester address",
            0x41 => "incorrect control word",
            0x42 => "incorrect format",
            0x43 => "incorrect destination address",
            0x44 => "message too large",
            0x45 => "diagnostic application not ready",
            0xff => "out of memory",
            _ => return None,
        })
    }
}

mod doip {
    pub const HEADER: usize = 8;
    pub const VERSION: u8 = 0x02;
    pub const NACK: u16 = 0x0000;
    pub const IDENTIFICATION_REQUEST: u16 = 0x0001;
    pub const ROUTING_REQUEST: u16 = 0x0005;
    pub const ROUTING_RESPONSE: u16 = 0x0006;
    pub const ALIVE_REQUEST: u16 = 0x0007;
    pub const ALIVE_RESPONSE: u16 = 0x0008;
    pub const DIAGNOSTIC: u16 = 0x8001;
    pub const DIAGNOSTIC_NACK: u16 = 0x8003;
    pub const ROUTING_ACCEPTED: u8 = 0x10;
}

fn doip_message(kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(doip::HEADER + payload.len());
    out.extend_from_slice(&[doip::VERSION, !doip::VERSION]);
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The UDP broadcast that makes every DoIP entity on the network announce itself.
pub fn doip_identification_request() -> Vec<u8> {
    doip_message(doip::IDENTIFICATION_REQUEST, &[])
}

impl Framing {
    pub const fn name(self) -> &'static str {
        match self {
            Framing::Hsfz => "hsfz",
            Framing::Doip => "doip",
        }
    }

    pub const fn port(self) -> u16 {
        match self {
            Framing::Hsfz => 6801,
            Framing::Doip => 13400,
        }
    }

    /// Our own address on the diagnostic bus.
    pub const fn tester(self) -> u16 {
        match self {
            Framing::Hsfz => 0xf4,
            Framing::Doip => 0x0ef8,
        }
    }

    /// Highest module address the framing can express.
    pub const fn max_ecu(self) -> u16 {
        match self {
            Framing::Hsfz => 0xff,
            Framing::Doip => 0xffff,
        }
    }

    /// Sent right after the TCP connect, if the framing has a handshake.
    pub fn hello(self) -> Option<Vec<u8>> {
        match self {
            Framing::Hsfz => None,
            // Source address, activation type "default", four reserved bytes.
            Framing::Doip => {
                let [hi, lo] = self.tester().to_be_bytes();
                Some(doip_message(doip::ROUTING_REQUEST, &[hi, lo, 0, 0, 0, 0, 0]))
            }
        }
    }

    pub fn alive_reply(self) -> Option<Vec<u8>> {
        match self {
            Framing::Hsfz => None,
            Framing::Doip => Some(doip_message(doip::ALIVE_RESPONSE, &self.tester().to_be_bytes())),
        }
    }

    /// Wraps a UDS message. Addresses above [`max_ecu`](Self::max_ecu) are truncated.
    pub fn frame(self, src: u16, dst: u16, uds: &[u8]) -> Vec<u8> {
        match self {
            Framing::Hsfz => {
                let mut out = Vec::with_capacity(hsfz::HEADER + 2 + uds.len());
                out.extend_from_slice(&((uds.len() + 2) as u32).to_be_bytes());
                out.extend_from_slice(&hsfz::DIAGNOSTIC.to_be_bytes());
                out.extend_from_slice(&[src as u8, dst as u8]);
                out.extend_from_slice(uds);
                out
            }
            Framing::Doip => {
                let mut payload = Vec::with_capacity(4 + uds.len());
                payload.extend_from_slice(&src.to_be_bytes());
                payload.extend_from_slice(&dst.to_be_bytes());
                payload.extend_from_slice(uds);
                doip_message(doip::DIAGNOSTIC, &payload)
            }
        }
    }

    /// Takes one message off the front of `buf`. Returns the message and how
    /// many bytes it used, or `None` while the message is still incomplete.
    pub fn parse(self, buf: &[u8]) -> Result<Option<(Incoming, usize)>> {
        match self {
            Framing::Hsfz => parse_hsfz(buf),
            Framing::Doip => parse_doip(buf),
        }
    }
}

impl fmt::Display for Framing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Framing {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// `length(4) type(2) | src(1) dst(1) uds…`, the length counting everything after the type.
fn parse_hsfz(buf: &[u8]) -> Result<Option<(Incoming, usize)>> {
    let (Some(len), Some(kind)) = (be_u32(buf, 0), be_u16(buf, 4)) else {
        return Ok(None);
    };
    if len > MAX_PAYLOAD {
        return Err(Error::Protocol(format!("hsfz: frame of {len} bytes")));
    }
    let size = hsfz::HEADER + len as usize;
    let Some(body) = buf.get(hsfz::HEADER..size) else {
        return Ok(None);
    };

    let msg = if kind == hsfz::DIAGNOSTIC {
        let [src, dst, ref uds @ ..] = *body else {
            return Err(Error::Protocol("hsfz: diagnostic frame without addresses".into()));
        };
        if uds.is_empty() {
            return Err(Error::Protocol("hsfz: empty diagnostic frame".into()));
        }
        Incoming::Uds { src: src.into(), dst: dst.into(), uds: uds.to_vec() }
    } else if let Some(reason) = hsfz::error(kind) {
        Incoming::Refused(format!("hsfz: {reason}"))
    } else {
        // 0x02 echoes our request back as an acknowledgement, 0x12 is an alive check.
        Incoming::Ignored
    };
    Ok(Some((msg, size)))
}

/// `version(1) !version(1) type(2) length(4) | payload`
fn parse_doip(buf: &[u8]) -> Result<Option<(Incoming, usize)>> {
    if buf.len() < doip::HEADER {
        return Ok(None);
    }
    if buf[0] != !buf[1] {
        return Err(Error::Protocol(format!("doip: bad version bytes {}", Hex(&buf[..2]))));
    }
    let (Some(kind), Some(len)) = (be_u16(buf, 2), be_u32(buf, 4)) else {
        return Ok(None);
    };
    if len > MAX_PAYLOAD {
        return Err(Error::Protocol(format!("doip: message of {len} bytes")));
    }
    let size = doip::HEADER + len as usize;
    let Some(body) = buf.get(doip::HEADER..size) else {
        return Ok(None);
    };

    let msg = match kind {
        doip::DIAGNOSTIC => match (be_u16(body, 0), be_u16(body, 2), body.get(4..)) {
            (Some(src), Some(dst), Some(uds)) if !uds.is_empty() => {
                Incoming::Uds { src, dst, uds: uds.to_vec() }
            }
            _ => return Err(Error::Protocol("doip: truncated diagnostic message".into())),
        },
        doip::ROUTING_RESPONSE => match (be_u16(body, 0), body.get(4)) {
            (Some(tester), Some(&code)) if tester == Framing::Doip.tester() => {
                if code == doip::ROUTING_ACCEPTED {
                    Incoming::Activated
                } else {
                    Incoming::Refused(format!("doip: routing activation refused ({code:#04x})"))
                }
            }
            _ => Incoming::Ignored,
        },
        doip::ALIVE_REQUEST => Incoming::AliveCheck,
        doip::NACK | doip::DIAGNOSTIC_NACK => Incoming::Refused(format!("doip: nack {}", Hex(body))),
        _ => Incoming::Ignored,
    };
    Ok(Some((msg, size)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::parse_hex;

    fn hex(s: &str) -> Vec<u8> {
        parse_hex(s).unwrap()
    }

    #[test]
    fn hsfz_frame() {
        let frame = Framing::Hsfz.frame(0xf4, 0x40, &hex("22 f1 90"));
        assert_eq!(Hex(&frame).to_string(), "00 00 00 05 00 01 f4 40 22 f1 90");
    }

    #[test]
    fn hsfz_waits_for_the_whole_frame() {
        let frame = hex("00 00 00 05 00 01 40 f4 62 f1 90");
        assert_eq!(Framing::Hsfz.parse(&frame[..8]).unwrap(), None);
        let (msg, size) = Framing::Hsfz.parse(&frame).unwrap().unwrap();
        assert_eq!(msg, Incoming::Uds { src: 0x40, dst: 0xf4, uds: hex("62 f1 90") });
        assert_eq!(size, 11);
    }

    #[test]
    fn hsfz_gateway_errors() {
        let (msg, _) = Framing::Hsfz.parse(&hex("00 00 00 02 00 43 f4 99")).unwrap().unwrap();
        assert_eq!(msg, Incoming::Refused("hsfz: incorrect destination address".into()));
    }

    #[test]
    fn hsfz_rejects_oversized_length() {
        assert!(Framing::Hsfz.parse(&hex("7f 00 00 00 00 01")).is_err());
    }

    #[test]
    fn doip_routing_activation() {
        let hello = Framing::Doip.hello().unwrap();
        assert_eq!(Hex(&hello).to_string(), "02 fd 00 05 00 00 00 07 0e f8 00 00 00 00 00");

        let accepted = hex("02 fd 00 06 00 00 00 09 0e f8 00 10 10 00 00 00 00");
        assert_eq!(Framing::Doip.parse(&accepted).unwrap().unwrap().0, Incoming::Activated);
        let refused = hex("02 fd 00 06 00 00 00 09 0e f8 00 10 06 00 00 00 00");
        assert!(matches!(Framing::Doip.parse(&refused).unwrap().unwrap().0, Incoming::Refused(_)));
    }

    #[test]
    fn doip_round_trip() {
        let frame = Framing::Doip.frame(0x0ef8, 0x40, &hex("22 f1 90"));
        assert_eq!(Hex(&frame).to_string(), "02 fd 80 01 00 00 00 07 0e f8 00 40 22 f1 90");
        let (msg, size) = Framing::Doip.parse(&frame).unwrap().unwrap();
        assert_eq!(msg, Incoming::Uds { src: 0x0ef8, dst: 0x40, uds: hex("22 f1 90") });
        assert_eq!(size, frame.len());
    }

    #[test]
    fn doip_rejects_garbage_version() {
        assert!(Framing::Doip.parse(&hex("02 02 80 01 00 00 00 00")).is_err());
    }

    #[test]
    fn doip_alive_check() {
        let (msg, _) = Framing::Doip.parse(&hex("02 fd 00 07 00 00 00 00")).unwrap().unwrap();
        assert_eq!(msg, Incoming::AliveCheck);
        assert_eq!(Hex(&Framing::Doip.alive_reply().unwrap()).to_string(), "02 fd 00 08 00 00 00 02 0e f8");
    }
}
