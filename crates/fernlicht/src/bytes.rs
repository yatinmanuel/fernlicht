//! Hex formatting and parsing for traces, reports and the command line.

use std::fmt;

use crate::Error;

/// Formats bytes as lowercase hex pairs separated by spaces: `22 f1 90`.
#[derive(Debug, Clone, Copy)]
pub struct Hex<'a>(pub &'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Parses hex bytes. Whitespace, commas and `0x` prefixes are ignored, so
/// `"22 f1 90"`, `"22f190"` and `"0x22, 0xF1, 0x90"` are all the same request.
pub fn parse_hex(input: &str) -> Result<Vec<u8>, Error> {
    let lower = input.to_ascii_lowercase().replace("0x", "");
    let digits: Vec<u8> = lower.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b',').collect();

    if digits.is_empty() || digits.len() % 2 != 0 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(Error::InvalidHex(input.to_owned()));
    }
    Ok(digits.chunks_exact(2).map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1])).collect())
}

fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        _ => digit.to_ascii_lowercase() - b'a' + 10,
    }
}

pub(crate) fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

pub(crate) fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let chunk = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_pairs() {
        assert_eq!(Hex(&[0x22, 0xf1, 0x05]).to_string(), "22 f1 05");
        assert_eq!(Hex(&[]).to_string(), "");
    }

    #[test]
    fn parses_loose_input() {
        assert_eq!(parse_hex("22 f1 90").unwrap(), [0x22, 0xf1, 0x90]);
        assert_eq!(parse_hex("0x22,0xF1, 0x90").unwrap(), [0x22, 0xf1, 0x90]);
        assert_eq!(parse_hex("2ed542").unwrap(), [0x2e, 0xd5, 0x42]);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["", "2", "zz", "22 f", "22 +1"] {
            assert!(parse_hex(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn reads_big_endian() {
        assert_eq!(be_u16(&[0xd5, 0x42], 0), Some(0xd542));
        assert_eq!(be_u16(&[0xd5], 0), None);
        assert_eq!(be_u32(&[0, 0, 1, 2], 0), Some(0x0102));
    }
}
