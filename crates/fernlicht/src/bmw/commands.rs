//! Request bytes for the three light commands. Everything here only builds
//! requests; nothing is sent.

use std::time::Duration;

use super::tables::{FLE_CHANNELS, FLE_ROUTINE, FemOutput, Lamp, RemOutput, did};
use crate::uds::{self, IoControl, Routine};

/// Highest FLE drive current known to be accepted. `0xff` is refused with
/// "request out of range"; the real limit lies somewhere in between.
pub const FLE_MAX_CURRENT: u8 = 0x32;

/// FEM: light `lamp` for `duration`, after which the FEM switches it off by
/// itself.
///
/// `2e d5 42 <lamp:2> <time:2>`, time in 10 ms ticks, saturating at about
/// eleven minutes. Both fields must be two bytes wide.
pub fn fem_lamp(lamp: Lamp, duration: Duration) -> Vec<u8> {
    lamp_function(lamp.code(), duration)
}

/// FEM: end every lamp function early.
pub fn fem_lamp_clear() -> Vec<u8> {
    lamp_function(0, Duration::ZERO)
}

fn lamp_function(code: u16, duration: Duration) -> Vec<u8> {
    let ticks = (duration.as_secs_f64() * 100.0).round().min(f64::from(u16::MAX)) as u16;
    let mut data = code.to_be_bytes().to_vec();
    data.extend_from_slice(&ticks.to_be_bytes());
    uds::write_did(did::LAMP_FUNCTION, &data)
}

/// FEM: force one output on or off until released or the session ends.
///
/// `2f 45 01 03 <output> <state>`
pub fn fem_output(output: FemOutput, on: bool) -> Vec<u8> {
    uds::io_control(did::LAMP_OUTPUT, IoControl::ShortTermAdjustment, &[output as u8, u8::from(on)])
}

/// FEM: hand a forced output back to the module. `2f 45 01 00 <output>`
pub fn fem_output_release(output: FemOutput) -> Vec<u8> {
    uds::io_control(did::LAMP_OUTPUT, IoControl::ReturnControl, &[output as u8])
}

/// REM: force a rear output. `2e 45 01 <output> <state>`
///
/// There is no release for this one, and a forced-off output does not
/// reliably recover when the session ends. To undo it, write the output back
/// on and then return to the default session.
pub fn rem_output(output: RemOutput, on: bool) -> Vec<u8> {
    uds::write_did(did::LAMP_OUTPUT, &[output as u8, u8::from(on)])
}

/// FLE: drive every LED channel of one headlight at `pwm` percent.
pub fn fle_leds(pwm: u8) -> Vec<u8> {
    fle_channels(&[pwm; FLE_CHANNELS], FLE_MAX_CURRENT)
}

/// FLE: drive each LED channel separately.
///
/// `31 01 30 00 (<current> <pwm>) × 10`. PWM is clamped to 100 and current to
/// [`FLE_MAX_CURRENT`]; channels missing from `pwm` are driven at zero. Which
/// channel feeds which LED group is not mapped yet.
pub fn fle_channels(pwm: &[u8], current: u8) -> Vec<u8> {
    let current = current.min(FLE_MAX_CURRENT);
    let data: Vec<u8> =
        (0..FLE_CHANNELS).flat_map(|ch| [current, pwm.get(ch).copied().unwrap_or(0).min(100)]).collect();
    uds::routine(Routine::Start, FLE_ROUTINE, &data)
}

/// FLE: stop the LED routine and return the headlight to normal operation.
pub fn fle_stop() -> Vec<u8> {
    uds::routine(Routine::Stop, FLE_ROUTINE, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::Hex;

    fn hex(bytes: &[u8]) -> String {
        Hex(bytes).to_string()
    }

    #[test]
    fn command_bytes() {
        assert_eq!(hex(&fem_lamp(Lamp::LowBeam, Duration::from_millis(200))), "2e d5 42 00 03 00 14");
        assert_eq!(hex(&fem_lamp_clear()), "2e d5 42 00 00 00 00");
        assert_eq!(hex(&fem_output(FemOutput::All, false)), "2f 45 01 03 fe 00");
        assert_eq!(hex(&fem_output_release(FemOutput::All)), "2f 45 01 00 fe");
        assert_eq!(hex(&rem_output(RemOutput::Plate, true)), "2e 45 01 22 01");
        assert_eq!(hex(&fle_stop()), "31 02 30 00");
    }

    #[test]
    fn lamp_time_saturates() {
        assert_eq!(hex(&fem_lamp(Lamp::Position, Duration::from_secs(100_000))[5..]), "ff ff");
    }

    #[test]
    fn fle_clamps_pwm_and_current() {
        let all = fle_channels(&[250; FLE_CHANNELS], 0xff);
        assert_eq!(all.len(), 4 + 2 * FLE_CHANNELS);
        assert_eq!(all[4..8], [0x32, 100, 0x32, 100]);

        let partial = fle_channels(&[10, 20], FLE_MAX_CURRENT);
        assert_eq!(partial[4..10], [0x32, 10, 0x32, 20, 0x32, 0]);
        assert_eq!(fle_leds(35)[5], 35);
    }
}
