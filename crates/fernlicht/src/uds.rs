//! Unified Diagnostic Services (ISO 14229-1): building requests and pairing
//! replies with them.

use std::fmt;

/// Service identifiers used here.
pub mod sid {
    pub const SESSION_CONTROL: u8 = 0x10;
    pub const READ_DTC: u8 = 0x19;
    pub const READ_DID: u8 = 0x22;
    pub const WRITE_DID: u8 = 0x2e;
    pub const IO_CONTROL: u8 = 0x2f;
    pub const ROUTINE_CONTROL: u8 = 0x31;
    pub const TESTER_PRESENT: u8 = 0x3e;
    pub const NEGATIVE_RESPONSE: u8 = 0x7f;
}

/// Offset between a request SID and its positive response SID.
const POSITIVE_OFFSET: u8 = 0x40;

/// Suppress-positive-response bit of a sub-function byte.
const SUPPRESS_REPLY: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Session {
    Default = 0x01,
    Extended = 0x03,
}

/// Control parameter of an input/output control request (0x2F).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IoControl {
    ReturnControl = 0x00,
    ShortTermAdjustment = 0x03,
}

/// Sub-function of a routine control request (0x31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Routine {
    Start = 0x01,
    Stop = 0x02,
    Results = 0x03,
}

pub fn session_control(session: Session) -> Vec<u8> {
    vec![sid::SESSION_CONTROL, session as u8]
}

/// With `suppress_reply` set the module does not answer, which is what a
/// keep-alive wants.
pub fn tester_present(suppress_reply: bool) -> Vec<u8> {
    vec![sid::TESTER_PRESENT, if suppress_reply { SUPPRESS_REPLY } else { 0 }]
}

pub fn read_did(did: u16) -> Vec<u8> {
    with_id(sid::READ_DID, &[], did, &[])
}

pub fn write_did(did: u16, data: &[u8]) -> Vec<u8> {
    with_id(sid::WRITE_DID, &[], did, data)
}

pub fn io_control(did: u16, control: IoControl, data: &[u8]) -> Vec<u8> {
    let mut request = with_id(sid::IO_CONTROL, &[], did, &[control as u8]);
    request.extend_from_slice(data);
    request
}

pub fn routine(control: Routine, id: u16, data: &[u8]) -> Vec<u8> {
    with_id(sid::ROUTINE_CONTROL, &[control as u8], id, data)
}

fn with_id(sid: u8, sub: &[u8], id: u16, data: &[u8]) -> Vec<u8> {
    let mut request = Vec::with_capacity(3 + sub.len() + data.len());
    request.push(sid);
    request.extend_from_slice(sub);
    request.extend_from_slice(&id.to_be_bytes());
    request.extend_from_slice(data);
    request
}

/// Negative response code.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nrc(pub u8);

impl Nrc {
    pub const GENERAL_REJECT: Nrc = Nrc(0x10);
    pub const SERVICE_NOT_SUPPORTED: Nrc = Nrc(0x11);
    pub const SUB_FUNCTION_NOT_SUPPORTED: Nrc = Nrc(0x12);
    pub const INCORRECT_LENGTH: Nrc = Nrc(0x13);
    pub const RESPONSE_TOO_LONG: Nrc = Nrc(0x14);
    pub const BUSY_REPEAT_REQUEST: Nrc = Nrc(0x21);
    pub const CONDITIONS_NOT_CORRECT: Nrc = Nrc(0x22);
    pub const REQUEST_SEQUENCE_ERROR: Nrc = Nrc(0x24);
    pub const REQUEST_OUT_OF_RANGE: Nrc = Nrc(0x31);
    pub const SECURITY_ACCESS_DENIED: Nrc = Nrc(0x33);
    pub const INVALID_KEY: Nrc = Nrc(0x35);
    pub const EXCEEDED_ATTEMPTS: Nrc = Nrc(0x36);
    pub const TIME_DELAY_NOT_EXPIRED: Nrc = Nrc(0x37);
    pub const PROGRAMMING_FAILURE: Nrc = Nrc(0x72);
    pub const RESPONSE_PENDING: Nrc = Nrc(0x78);
    pub const SUB_FUNCTION_NOT_IN_SESSION: Nrc = Nrc(0x7e);
    pub const SERVICE_NOT_IN_SESSION: Nrc = Nrc(0x7f);

    pub fn description(self) -> Option<&'static str> {
        Some(match self {
            Self::GENERAL_REJECT => "general reject",
            Self::SERVICE_NOT_SUPPORTED => "service not supported",
            Self::SUB_FUNCTION_NOT_SUPPORTED => "sub-function not supported",
            Self::INCORRECT_LENGTH => "incorrect message length or format",
            Self::RESPONSE_TOO_LONG => "response too long",
            Self::BUSY_REPEAT_REQUEST => "busy, repeat request",
            Self::CONDITIONS_NOT_CORRECT => "conditions not correct",
            Self::REQUEST_SEQUENCE_ERROR => "request sequence error",
            Self::REQUEST_OUT_OF_RANGE => "request out of range",
            Self::SECURITY_ACCESS_DENIED => "security access denied",
            Self::INVALID_KEY => "invalid key",
            Self::EXCEEDED_ATTEMPTS => "exceeded number of attempts",
            Self::TIME_DELAY_NOT_EXPIRED => "required time delay not expired",
            Self::PROGRAMMING_FAILURE => "general programming failure",
            Self::RESPONSE_PENDING => "response pending",
            Self::SUB_FUNCTION_NOT_IN_SESSION => "sub-function not supported in active session",
            Self::SERVICE_NOT_IN_SESSION => "service not supported in active session",
            _ => return None,
        })
    }
}

impl fmt::Debug for Nrc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Nrc({:#04x})", self.0)
    }
}

impl fmt::Display for Nrc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.description() {
            Some(text) => write!(f, "{text} (nrc {:02x})", self.0),
            None => write!(f, "nrc {:02x}", self.0),
        }
    }
}

/// A module's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `data` is everything after the response SID, echoed identifier included.
    Positive {
        sid: u8,
        data: Vec<u8>,
    },
    Negative {
        sid: u8,
        nrc: Nrc,
    },
}

impl Reply {
    /// Decodes a raw UDS response. `None` for an empty or truncated one.
    pub fn parse(raw: &[u8]) -> Option<Reply> {
        match *raw {
            [sid::NEGATIVE_RESPONSE, sid, nrc, ..] => Some(Reply::Negative { sid, nrc: Nrc(nrc) }),
            [sid::NEGATIVE_RESPONSE, ..] | [] => None,
            [first, ref data @ ..] => {
                Some(Reply::Positive { sid: first.wrapping_sub(POSITIVE_OFFSET), data: data.to_vec() })
            }
        }
    }

    pub fn is_positive(&self) -> bool {
        matches!(self, Reply::Positive { .. })
    }

    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Reply::Positive { data, .. } => Some(data),
            Reply::Negative { .. } => None,
        }
    }

    pub fn nrc(&self) -> Option<Nrc> {
        match self {
            Reply::Positive { .. } => None,
            Reply::Negative { nrc, .. } => Some(*nrc),
        }
    }
}

/// True for "response pending" (NRC 0x78): the real answer is still coming.
pub fn is_pending(raw: &[u8]) -> bool {
    matches!(raw, [sid::NEGATIVE_RESPONSE, _, 0x78, ..])
}

/// How many bytes after the SID a positive response echoes from the request.
fn echo_len(sid: u8) -> usize {
    match sid {
        sid::READ_DID | sid::WRITE_DID | sid::IO_CONTROL => 2,
        sid::ROUTINE_CONTROL => 3,
        sid::SESSION_CONTROL | sid::TESTER_PRESENT | sid::READ_DTC => 1,
        _ => 0,
    }
}

/// The part of a request that its positive response repeats. Two requests
/// with the same signature get replies that cannot be told apart.
pub fn signature(request: &[u8]) -> &[u8] {
    let Some(&sid) = request.first() else {
        return request;
    };
    &request[..request.len().min(1 + echo_len(sid))]
}

/// Whether `response` answers `request`.
///
/// The gateway multiplexes every module over one connection and a late reply
/// to an earlier request can look like a fresh one, so this compares the
/// echoed identifier as well as the service.
pub fn answers(request: &[u8], response: &[u8]) -> bool {
    let (Some(&service), Some(&head)) = (request.first(), response.first()) else {
        return false;
    };
    if head == sid::NEGATIVE_RESPONSE {
        return response.len() >= 3 && response[1] == service;
    }
    if head != service.wrapping_add(POSITIVE_OFFSET) {
        return false;
    }
    let n = echo_len(service);
    if request.len() <= n || response.len() <= n {
        return false;
    }
    (1..=n).all(|i| {
        // A sub-function comes back without its suppress-reply bit.
        let sent = if i == 1 && n != 2 { request[i] & !SUPPRESS_REPLY } else { request[i] };
        response[i] == sent
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::Hex;

    #[test]
    fn builds_requests() {
        assert_eq!(Hex(&read_did(0xf190)).to_string(), "22 f1 90");
        assert_eq!(Hex(&write_did(0xd542, &[0, 3])).to_string(), "2e d5 42 00 03");
        assert_eq!(
            Hex(&io_control(0x4501, IoControl::ShortTermAdjustment, &[0xfe, 0])).to_string(),
            "2f 45 01 03 fe 00"
        );
        assert_eq!(Hex(&routine(Routine::Stop, 0x3000, &[])).to_string(), "31 02 30 00");
        assert_eq!(Hex(&session_control(Session::Extended)).to_string(), "10 03");
        assert_eq!(Hex(&tester_present(true)).to_string(), "3e 80");
    }

    #[test]
    fn parses_replies() {
        assert_eq!(
            Reply::parse(&[0x62, 0xf1, 0x90]),
            Some(Reply::Positive { sid: 0x22, data: vec![0xf1, 0x90] })
        );
        assert_eq!(Reply::parse(&[0x7f, 0x22, 0x31]), Some(Reply::Negative { sid: 0x22, nrc: Nrc(0x31) }));
        assert_eq!(Reply::parse(&[0x7f, 0x22]), None);
        assert_eq!(Reply::parse(&[]), None);
    }

    #[test]
    fn matches_on_echoed_identifier() {
        let request = read_did(0xf190);
        assert!(answers(&request, &[0x62, 0xf1, 0x90, 0x41]));
        assert!(answers(&request, &[0x7f, 0x22, 0x31]));
        assert!(!answers(&request, &[0x62, 0xf1, 0x91, 0x41]));
        assert!(!answers(&request, &[0x6e, 0xf1, 0x90]));
        assert!(!answers(&request, &[0x7f, 0x2e, 0x31]));
    }

    #[test]
    fn sub_function_echo_ignores_suppress_bit() {
        assert!(answers(&tester_present(true), &[0x7e, 0x00]));
        assert!(answers(&routine(Routine::Start, 0x3000, &[1]), &[0x71, 0x01, 0x30, 0x00]));
        assert!(!answers(&routine(Routine::Start, 0x3000, &[1]), &[0x71, 0x02, 0x30, 0x00]));
    }

    #[test]
    fn signature_covers_the_echo() {
        assert_eq!(signature(&routine(Routine::Start, 0x3000, &[1, 2])), [0x31, 0x01, 0x30, 0x00]);
        assert_eq!(signature(&read_did(0xf190)), [0x22, 0xf1, 0x90]);
        assert_eq!(signature(&[0x11, 0x01]), [0x11]);
    }

    #[test]
    fn describes_nrc() {
        assert_eq!(Nrc::CONDITIONS_NOT_CORRECT.to_string(), "conditions not correct (nrc 22)");
        assert_eq!(Nrc(0x99).to_string(), "nrc 99");
        assert!(is_pending(&[0x7f, 0x31, 0x78]));
    }
}
