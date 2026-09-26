use std::fmt;
use std::str::FromStr;

use crate::bmw::ParseNameError;

/// One light of the car, as far as a show is concerned. Front outputs are
/// `Fl*`/`Fr*`, rear outputs `Rl*`/`Rr*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Output {
    FlDrl,
    FlLow,
    FlHigh,
    FlTurn,
    FlRing,
    FrDrl,
    FrLow,
    FrHigh,
    FrTurn,
    FrRing,
    RlTail,
    RlBrake,
    RlTurn,
    RrTail,
    RrBrake,
    RrTurn,
}

impl Output {
    pub const COUNT: usize = 16;

    pub const ALL: [Output; Output::COUNT] = [
        Output::FlDrl,
        Output::FlLow,
        Output::FlHigh,
        Output::FlTurn,
        Output::FlRing,
        Output::FrDrl,
        Output::FrLow,
        Output::FrHigh,
        Output::FrTurn,
        Output::FrRing,
        Output::RlTail,
        Output::RlBrake,
        Output::RlTurn,
        Output::RrTail,
        Output::RrBrake,
        Output::RrTurn,
    ];

    /// The name used in show files, e.g. `fl_drl`.
    pub const fn name(self) -> &'static str {
        match self {
            Output::FlDrl => "fl_drl",
            Output::FlLow => "fl_low",
            Output::FlHigh => "fl_high",
            Output::FlTurn => "fl_turn",
            Output::FlRing => "fl_ring",
            Output::FrDrl => "fr_drl",
            Output::FrLow => "fr_low",
            Output::FrHigh => "fr_high",
            Output::FrTurn => "fr_turn",
            Output::FrRing => "fr_ring",
            Output::RlTail => "rl_tail",
            Output::RlBrake => "rl_brake",
            Output::RlTurn => "rl_turn",
            Output::RrTail => "rr_tail",
            Output::RrBrake => "rr_brake",
            Output::RrTurn => "rr_turn",
        }
    }

    pub const fn is_front(self) -> bool {
        (self as usize) < Output::RlTail as usize
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Output {
    type Err = ParseNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Output::ALL
            .into_iter()
            .find(|output| output.name() == s)
            .ok_or_else(|| ParseNameError::new("output", s))
    }
}

/// Brightness changes for some outputs, 0 to 255. Outputs left unset keep
/// whatever level they had.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Levels([Option<u8>; Output::COUNT]);

impl Levels {
    pub const fn new() -> Self {
        Self([None; Output::COUNT])
    }

    /// Every output at `level`.
    pub fn all(level: u8) -> Self {
        Self([Some(level); Output::COUNT])
    }

    #[must_use]
    pub fn with(mut self, outputs: &[Output], level: u8) -> Self {
        for &output in outputs {
            self.set(output, level);
        }
        self
    }

    pub fn set(&mut self, output: Output, level: u8) {
        self.0[output.index()] = Some(level);
    }

    pub fn get(&self, output: Output) -> Option<u8> {
        self.0[output.index()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (Output, u8)> + '_ {
        Output::ALL.into_iter().filter_map(|output| Some((output, self.get(output)?)))
    }
}

/// The level of every output at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lights([u8; Output::COUNT]);

impl Lights {
    pub const fn dark() -> Self {
        Self([0; Output::COUNT])
    }

    pub fn get(&self, output: Output) -> u8 {
        self.0[output.index()]
    }

    pub fn apply(&mut self, levels: &Levels) {
        for (output, level) in levels.iter() {
            self.0[output.index()] = level;
        }
    }

    /// The brightest of `outputs`.
    pub fn max_of(&self, outputs: &[Output]) -> u8 {
        outputs.iter().map(|&o| self.get(o)).max().unwrap_or(0)
    }
}
