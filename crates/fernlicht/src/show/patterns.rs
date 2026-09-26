//! Building blocks for shows. Each returns the steps of one pass; shows
//! concatenate them.

use super::{Levels, Output, Step};

/// Every output switched on and off `times` times.
pub fn flash(outputs: &[Output], times: usize, on_ms: u64, off_ms: u64) -> Vec<Step> {
    let on = Step::new(Levels::new().with(outputs, 255), on_ms);
    let off = Step::new(Levels::new().with(outputs, 0), off_ms);
    [on, off].repeat(times)
}

/// One output lit at a time, in order.
pub fn chase(outputs: &[Output], on_ms: u64) -> Vec<Step> {
    let dark = Levels::new().with(outputs, 0);
    outputs.iter().map(|&output| Step::new(dark.with(&[output], 255), on_ms)).collect()
}

/// Fades up and back down.
pub fn breathe(outputs: &[Output], step_ms: u64) -> Vec<Step> {
    let up = ramp(17);
    up.iter()
        .chain(up.iter().rev())
        .map(|&level| Step::new(Levels::new().with(outputs, level), step_ms))
        .collect()
}

/// Fades up, holds at full, fades out.
pub fn swell(outputs: &[Output], step_ms: u64, hold_ms: u64) -> Vec<Step> {
    let up: Vec<Step> =
        ramp(15).into_iter().map(|level| Step::new(Levels::new().with(outputs, level), step_ms)).collect();
    let mut steps = up.clone();
    steps.push(Step::new(Levels::new().with(outputs, 255), hold_ms));
    steps.extend(up.into_iter().rev());
    steps
}

/// `a` on, gap, `b` on, gap.
pub fn alternate(a: &[Output], b: &[Output], on_ms: u64, gap_ms: u64) -> Vec<Step> {
    let off = Levels::new().with(a, 0).with(b, 0);
    vec![
        Step::new(off.with(a, 255), on_ms),
        Step::new(off, gap_ms),
        Step::new(off.with(b, 255), on_ms),
        Step::new(off, gap_ms),
    ]
}

/// `a` fades up while `b` fades down, then the other way round.
pub fn crossfade(a: &[Output], b: &[Output], count: usize, step_ms: u64) -> Vec<Step> {
    let last = count.saturating_sub(1).max(1) as f64;
    (0..count)
        .map(|i| {
            let t = i as f64 / last;
            let level = ((if t < 0.5 { t } else { 1.0 - t }) * 2.0 * 255.0).round() as u8;
            Step::new(Levels::new().with(a, level).with(b, 255 - level), step_ms)
        })
        .collect()
}

/// 0, `step`, 2·`step`, … up to and including 255 when it divides evenly.
fn ramp(step: u8) -> Vec<u8> {
    (0..=255).step_by(step.into()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_end_at_full() {
        assert_eq!(ramp(17).last(), Some(&255));
        assert_eq!(ramp(15).len(), 18);
    }

    #[test]
    fn chase_lights_one_at_a_time() {
        let steps = chase(&[Output::FlTurn, Output::FrTurn], 100);
        assert_eq!(steps[0].levels.get(Output::FlTurn), Some(255));
        assert_eq!(steps[0].levels.get(Output::FrTurn), Some(0));
        assert_eq!(steps[1].levels.get(Output::FlTurn), Some(0));
    }

    #[test]
    fn crossfade_is_symmetric() {
        let steps = crossfade(&[Output::FlDrl], &[Output::FrDrl], 24, 45);
        let left: Vec<u8> = steps.iter().filter_map(|s| s.levels.get(Output::FlDrl)).collect();
        assert_eq!(left.first(), Some(&0));
        assert_eq!(left.last(), Some(&0));
        assert!(left.iter().zip(left.iter().rev()).all(|(a, b)| a == b));
    }
}
