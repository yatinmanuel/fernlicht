//! Addresses, identifiers and element tables for `FEM_20`, `FLE02` and `REM_20`.
//! Names in the comments are the jobs and tables of BMW's ECU description files.

use std::fmt;
use std::str::FromStr;

/// Module addresses behind the gateway.
pub mod ecu {
    pub const GATEWAY: u16 = 0x10;
    /// Front electronic module, master for the exterior lights.
    pub const FEM: u16 = 0x40;
    pub const FLE_LEFT: u16 = 0x43;
    pub const FLE_RIGHT: u16 = 0x44;
    pub const KOMBI: u16 = 0x60;
    /// Rear electronic module.
    pub const REM: u16 = 0x72;

    /// Where the lighting modules of an F3x/F8x live.
    pub const LIGHTING: [u16; 6] = [GATEWAY, FEM, FLE_LEFT, FLE_RIGHT, KOMBI, REM];
}

/// Data identifiers.
pub mod did {
    pub const VIN: u16 = 0xf190;
    pub const ECU_NAME: u16 = 0xf197;
    pub const HW_NUMBER: u16 = 0xf191;
    pub const SW_VERSION: u16 = 0xf189;
    /// Which ECU description file the module belongs to.
    pub const SGBD_INDEX: u16 = 0xf150;
    /// Terminal 30 voltage in 0.1 V, on the FEM.
    pub const VOLTAGE: u16 = 0xdad6;
    /// `LEUCHTEN_FUNKTION`: light a lamp function for a given time.
    pub const LAMP_FUNCTION: u16 = 0xd542;
    /// `STEUERN_LEUCHTENAUSGANG_DIGITAL`: force one output.
    pub const LAMP_OUTPUT: u16 = 0x4501;
}

/// FLE routine `_LEUCHTEN_AUSSENLICHT_KANAL`: current and PWM per LED channel.
pub const FLE_ROUTINE: u16 = 0x3000;
pub const FLE_CHANNELS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseNameError {
    kind: &'static str,
    name: String,
}

impl ParseNameError {
    pub(crate) fn new(kind: &'static str, name: &str) -> Self {
        Self { kind, name: name.to_owned() }
    }
}

impl fmt::Display for ParseNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} {:?}", self.kind, self.name)
    }
}

impl std::error::Error for ParseNameError {}

/// A lamp function of the FEM (`TAB_LAMPEN_FUNKTION`). A function covers the
/// whole car: [`Lamp::HighBeam`] lights both high beams, [`Lamp::TurnLeft`]
/// every left indicator front and rear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Lamp {
    Position = 0x01,
    LowBeam = 0x03,
    Drl = 0x04,
    HighBeam = 0x05,
    TurnLeft = 0x06,
    TurnRight = 0x07,
    FogFront = 0x08,
    CornerLeft = 0x09,
    CornerRight = 0x0a,
    Brake = 0x0c,
    FogRear = 0x0e,
    Reverse = 0x0f,
    ParkLeft = 0x10,
    ParkRight = 0x11,
    Hazards = 0x12,
    Interior = 0x13,
}

impl Lamp {
    pub const ALL: [Lamp; 16] = [
        Lamp::Position,
        Lamp::LowBeam,
        Lamp::Drl,
        Lamp::HighBeam,
        Lamp::TurnLeft,
        Lamp::TurnRight,
        Lamp::FogFront,
        Lamp::CornerLeft,
        Lamp::CornerRight,
        Lamp::Brake,
        Lamp::FogRear,
        Lamp::Reverse,
        Lamp::ParkLeft,
        Lamp::ParkRight,
        Lamp::Hazards,
        Lamp::Interior,
    ];

    pub const fn code(self) -> u16 {
        self as u16
    }

    pub const fn name(self) -> &'static str {
        match self {
            Lamp::Position => "position",
            Lamp::LowBeam => "low-beam",
            Lamp::Drl => "drl",
            Lamp::HighBeam => "high-beam",
            Lamp::TurnLeft => "turn-left",
            Lamp::TurnRight => "turn-right",
            Lamp::FogFront => "fog-front",
            Lamp::CornerLeft => "corner-left",
            Lamp::CornerRight => "corner-right",
            Lamp::Brake => "brake",
            Lamp::FogRear => "fog-rear",
            Lamp::Reverse => "reverse",
            Lamp::ParkLeft => "park-left",
            Lamp::ParkRight => "park-right",
            Lamp::Hazards => "hazards",
            Lamp::Interior => "interior",
        }
    }
}

impl fmt::Display for Lamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Lamp {
    type Err = ParseNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Lamp::ALL.into_iter().find(|lamp| lamp.name() == s).ok_or_else(|| ParseNameError::new("lamp", s))
    }
}

/// A single FEM output (`TAB_AUSGANG_LEUCHTEN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FemOutput {
    LowLeft = 0x01,
    LowRight = 0x02,
    DrlLeft = 0x03,
    DrlRight = 0x04,
    SideLeft = 0x05,
    SideRight = 0x06,
    HighLeft = 0x07,
    HighRight = 0x08,
    ParkLeft = 0x09,
    ParkRight = 0x0a,
    FogLeft = 0x0b,
    FogRight = 0x0c,
    BixenonLeft = 0x12,
    BixenonRight = 0x13,
    RingLeft = 0x30,
    RingRight = 0x31,
    All = 0xfe,
}

/// A single REM output (`LAMPEN_AUSGANG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RemOutput {
    TailLeft = 0x14,
    TailRight = 0x15,
    Tail2Left = 0x16,
    Tail2Right = 0x17,
    BrakeLeft = 0x18,
    BrakeRight = 0x19,
    BrakeForceLeft = 0x1a,
    BrakeForceRight = 0x1b,
    FogLeft = 0x1c,
    FogRight = 0x1d,
    ReverseLeft = 0x1e,
    ReverseRight = 0x1f,
    TurnLeft = 0x20,
    TurnRight = 0x21,
    Plate = 0x22,
    BrakeCenter = 0x23,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamp_names_round_trip() {
        for lamp in Lamp::ALL {
            assert_eq!(lamp.name().parse::<Lamp>(), Ok(lamp));
        }
        assert!("lowBeam".parse::<Lamp>().is_err());
    }
}
