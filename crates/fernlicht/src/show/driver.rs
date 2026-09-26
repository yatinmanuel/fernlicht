use std::fmt;
use std::time::Duration;

use super::{Lights, Output, Show, Via};
use crate::bmw::{Lamp, Profile, fem_lamp, fem_lamp_clear, fle_leds, fle_stop};
use crate::transport::UdsLink;
use crate::uds::{Session, session_control};
use crate::{Error, Result};

/// Turns the state of the lights into requests. [`Player`](super::Player)
/// calls `release` after `begin`, whatever happens in between.
pub trait Driver {
    fn begin(&mut self) -> Result<()>;
    fn frame(&mut self, lights: &Lights, hold: Duration) -> Result<()>;
    fn release(&mut self) -> Result<()>;
}

impl<D: Driver + ?Sized> Driver for Box<D> {
    fn begin(&mut self) -> Result<()> {
        (**self).begin()
    }
    fn frame(&mut self, lights: &Lights, hold: Duration) -> Result<()> {
        (**self).frame(lights, hold)
    }
    fn release(&mut self) -> Result<()> {
        (**self).release()
    }
}

/// Some modules take the light commands in the default session and refuse to
/// switch, so a failed session change is no reason to give up.
fn enter_extended_session(link: &impl UdsLink, ecu: u16) {
    let _ = link.send(ecu, &session_control(Session::Extended));
}

/// The FEM only switches, so a level above this counts as on.
const ON_THRESHOLD: u8 = 20;

/// Lamp functions cover the whole car, so asking for one high beam lights both.
const LAMP_OUTPUTS: [(Lamp, &[Output]); 7] = [
    (Lamp::HighBeam, &[Output::FlHigh, Output::FrHigh]),
    (Lamp::LowBeam, &[Output::FlLow, Output::FrLow]),
    (Lamp::Drl, &[Output::FlDrl, Output::FrDrl, Output::FlRing, Output::FrRing]),
    (Lamp::TurnLeft, &[Output::FlTurn, Output::RlTurn]),
    (Lamp::TurnRight, &[Output::FrTurn, Output::RrTurn]),
    (Lamp::Brake, &[Output::RlBrake, Output::RrBrake]),
    (Lamp::Position, &[Output::RlTail, Output::RrTail]),
];

/// A lamp function must outlast its step, or a lamp that stays on flickers
/// between frames.
const LAMP_MIN: Duration = Duration::from_millis(60);
const LAMP_OVERLAP: Duration = Duration::from_millis(60);

/// Drives the whole car through the FEM's lamp functions.
#[derive(Debug)]
pub struct FemDriver<L> {
    link: L,
    body: u16,
}

impl<L: UdsLink> FemDriver<L> {
    pub fn new(link: L, body: u16) -> Self {
        Self { link, body }
    }
}

impl<L: UdsLink> Driver for FemDriver<L> {
    fn begin(&mut self) -> Result<()> {
        enter_extended_session(&self.link, self.body);
        Ok(())
    }

    fn frame(&mut self, lights: &Lights, hold: Duration) -> Result<()> {
        let duration = hold.max(LAMP_MIN) + LAMP_OVERLAP;
        for (lamp, outputs) in LAMP_OUTPUTS {
            if lights.max_of(outputs) > ON_THRESHOLD {
                self.link.send(self.body, &fem_lamp(lamp, duration))?;
            }
        }
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.link.send(self.body, &fem_lamp_clear()).map(drop)
    }
}

const LEFT: [Output; 4] = [Output::FlDrl, Output::FlLow, Output::FlHigh, Output::FlRing];
const RIGHT: [Output; 4] = [Output::FrDrl, Output::FrLow, Output::FrHigh, Output::FrRing];

fn pwm(lights: &Lights, side: &[Output]) -> u8 {
    (f64::from(lights.max_of(side)) / 255.0 * 100.0).round() as u8
}

/// Dims the two LED headlights. Each side follows its brightest output,
/// since the LED channels are not mapped to lamps yet.
#[derive(Debug)]
pub struct FleDriver<L> {
    link: L,
    left: u16,
    right: u16,
}

impl<L: UdsLink> FleDriver<L> {
    pub fn new(link: L, left: u16, right: u16) -> Self {
        Self { link, left, right }
    }
}

impl<L: UdsLink> Driver for FleDriver<L> {
    fn begin(&mut self) -> Result<()> {
        enter_extended_session(&self.link, self.left);
        enter_extended_session(&self.link, self.right);
        Ok(())
    }

    fn frame(&mut self, lights: &Lights, _hold: Duration) -> Result<()> {
        self.link.send(self.left, &fle_leds(pwm(lights, &LEFT)))?;
        self.link.send(self.right, &fle_leds(pwm(lights, &RIGHT)))?;
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        // Stop both even if the first fails: a headlight left in the routine
        // is worse than an error.
        let left = self.link.send(self.left, &fle_stop());
        let right = self.link.send(self.right, &fle_stop());
        left.and(right).map(drop)
    }
}

/// Picks the driver a show needs, or fails if the car lacks the modules.
pub fn driver_for<'a, L: UdsLink + 'a>(
    show: &Show,
    link: L,
    profile: &Profile,
) -> Result<Box<dyn Driver + 'a>> {
    let missing = |module| Error::MissingModule { show: show.id.clone(), module };
    match show.via {
        Via::Fem => {
            let body = profile.body.ok_or_else(|| missing("a FEM_20"))?;
            Ok(Box::new(FemDriver::new(link, body)))
        }
        Via::Fle => match (profile.left, profile.right) {
            (Some(left), Some(right)) => Ok(Box::new(FleDriver::new(link, left, right))),
            _ => Err(missing("both FLE02 headlight modules")),
        },
    }
}

impl fmt::Debug for dyn Driver + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("dyn Driver")
    }
}
