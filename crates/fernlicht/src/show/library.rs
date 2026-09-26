use std::time::Duration;

use super::patterns::{alternate, breathe, chase, crossfade, flash, swell};
use super::{Levels, Output};

/// How a show reaches the lamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize), serde(rename_all = "lowercase"))]
pub enum Via {
    /// Lamp functions through the FEM: on or off only, but every lamp on the car.
    Fem,
    /// PWM through the two headlight modules: dimmable, front only.
    Fle,
}

impl Via {
    pub const fn name(self) -> &'static str {
        match self {
            Via::Fem => "fem",
            Via::Fle => "fle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub levels: Levels,
    pub hold: Duration,
}

impl Step {
    pub fn new(levels: Levels, hold_ms: u64) -> Self {
        Self { levels, hold: Duration::from_millis(hold_ms) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Show {
    pub id: String,
    pub name: String,
    pub via: Via,
    /// Start over after the last step until stopped.
    pub repeat: bool,
    pub steps: Vec<Step>,
}

impl Show {
    fn new(id: &str, name: &str, via: Via, repeat: bool, steps: Vec<Step>) -> Self {
        Self { id: id.to_owned(), name: name.to_owned(), via, repeat, steps }
    }
}

use Output::{FlDrl, FlHigh, FlLow, FlTurn, FrDrl, FrHigh, FrLow, FrTurn, RlTail, RlTurn, RrTail, RrTurn};

const HEADLIGHTS: [Output; 4] = [FlLow, FlHigh, FrLow, FrHigh];
const HIGH_BEAMS: [Output; 2] = [FlHigh, FrHigh];
const DRLS: [Output; 2] = [FlDrl, FrDrl];
const HEAD_LEFT: [Output; 2] = [FlDrl, FlLow];
const HEAD_RIGHT: [Output; 2] = [FrDrl, FrLow];
const TURNS_LEFT: [Output; 2] = [FlTurn, RlTurn];
const TURNS_RIGHT: [Output; 2] = [FrTurn, RrTurn];
/// Clockwise around the car, seen from above.
const TURNS_CLOCKWISE: [Output; 4] = [FlTurn, FrTurn, RrTurn, RlTurn];
const FRONT_TO_BACK: [Output; 6] = [FlDrl, FrDrl, FlLow, FrLow, RlTail, RrTail];

/// The shows that ship with the library.
pub fn builtin() -> Vec<Show> {
    let all = Output::ALL;
    let dark = Levels::all(0);
    let sweep: Vec<Output> = TURNS_CLOCKWISE.iter().chain(&[RrTurn, FrTurn]).copied().collect();

    let mut welcome = chase(&FRONT_TO_BACK, 160);
    welcome.push(Step::new(Levels::all(255), 600));
    welcome.push(Step::new(dark, 0));

    let mut lumen = breathe(&DRLS, 40);
    lumen.extend(flash(&HEADLIGHTS, 2, 80, 80));

    vec![
        Show::new("flash2", "Twin flash", Via::Fem, false, flash(&all, 2, 120, 120)),
        Show::new("flash3", "Triple flash", Via::Fem, false, flash(&all, 3, 120, 120)),
        Show::new("head-flash", "Headlight flash", Via::Fem, false, flash(&HEADLIGHTS, 3, 120, 120)),
        Show::new("welcome", "Welcome", Via::Fem, false, welcome),
        Show::new(
            "crossfire",
            "Crossfire",
            Via::Fem,
            true,
            vec![
                Step::new(Levels::new().with(&DRLS, 0).with(&HIGH_BEAMS, 255), 120),
                Step::new(Levels::new().with(&DRLS, 255).with(&HIGH_BEAMS, 0), 120),
            ],
        ),
        Show::new("side-wink", "Side wink", Via::Fem, true, alternate(&TURNS_RIGHT, &TURNS_LEFT, 600, 250)),
        Show::new("orbit", "Orbit", Via::Fem, true, chase(&TURNS_CLOCKWISE, 520)),
        Show::new("scanner", "Scanner", Via::Fem, true, chase(&sweep, 520)),
        Show::new("runway", "Runway", Via::Fem, true, chase(&FRONT_TO_BACK, 90)),
        Show::new("drift", "Drift", Via::Fem, true, chase(&FRONT_TO_BACK, 300)),
        Show::new(
            "carnival",
            "Carnival",
            Via::Fem,
            true,
            vec![
                Step::new(dark.with(&[FlDrl, RrTail], 255), 150),
                Step::new(dark.with(&[FrDrl, RlTail], 255), 150),
            ],
        ),
        Show::new("breathe", "Breathe", Via::Fle, true, breathe(&DRLS, 60)),
        Show::new("ember", "Ember", Via::Fle, true, breathe(&DRLS, 120)),
        Show::new("pulse", "Pulse", Via::Fle, true, breathe(&HEADLIGHTS, 110)),
        Show::new("sunrise", "Sunrise", Via::Fle, true, swell(&DRLS, 90, 1600)),
        Show::new("glide", "Glide", Via::Fle, true, crossfade(&[FlDrl], &[FrDrl], 48, 110)),
        Show::new("seesaw", "Seesaw", Via::Fle, true, crossfade(&HEAD_LEFT, &HEAD_RIGHT, 24, 45)),
        Show::new(
            "head-wink",
            "Headlight wink",
            Via::Fle,
            true,
            alternate(&HEAD_RIGHT, &HEAD_LEFT, 300, 160),
        ),
        Show::new("split-strobe", "Split strobe", Via::Fle, true, alternate(&HEAD_LEFT, &HEAD_RIGHT, 90, 40)),
        Show::new("lumen", "Lumen", Via::Fle, true, lumen),
    ]
}

/// A built-in show by id.
pub fn find(id: &str) -> Option<Show> {
    builtin().into_iter().find(|show| show.id == id)
}

#[cfg(feature = "serde")]
pub use spec::{ShowSpec, StepSpec};

#[cfg(feature = "serde")]
mod spec {
    use std::collections::BTreeMap;

    use super::{Show, Step, Via};
    use crate::Error;
    use crate::show::{Levels, Output};

    const MAX_STEPS: usize = 2000;
    const HOLD_MS: std::ops::RangeInclusive<u64> = 20..=60_000;

    /// A show as written in a file. Convert with `Show::try_from`, which
    /// validates it before it gets anywhere near the car.
    ///
    /// ```json
    /// { "id": "mine", "name": "Mine", "via": "fle", "loop": true,
    ///   "steps": [{ "levels": { "fl_drl": 255, "fr_drl": 0 }, "holdMs": 200 }] }
    /// ```
    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ShowSpec {
        pub id: String,
        pub name: String,
        pub via: Via,
        #[serde(default, rename = "loop")]
        pub repeat: bool,
        pub steps: Vec<StepSpec>,
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    pub struct StepSpec {
        #[serde(default)]
        pub levels: BTreeMap<String, u8>,
        pub hold_ms: u64,
    }

    impl TryFrom<ShowSpec> for Show {
        type Error = Error;

        fn try_from(spec: ShowSpec) -> Result<Self, Error> {
            let invalid = |reason: String| Error::InvalidShow(reason);
            let id_ok = (1..=40).contains(&spec.id.len())
                && spec.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !id_ok {
                return Err(invalid(format!("id {:?}", spec.id)));
            }
            let name = spec.name.trim();
            if name.is_empty() || name.len() > 100 {
                return Err(invalid("name must be 1 to 100 characters".into()));
            }
            if spec.steps.is_empty() || spec.steps.len() > MAX_STEPS {
                return Err(invalid(format!("needs 1 to {MAX_STEPS} steps")));
            }

            let mut steps = Vec::with_capacity(spec.steps.len());
            for (i, step) in spec.steps.into_iter().enumerate() {
                if !HOLD_MS.contains(&step.hold_ms) {
                    return Err(invalid(format!("step {i}: holdMs must be 20 to 60000")));
                }
                let mut levels = Levels::new();
                for (output, level) in &step.levels {
                    let output: Output = output.parse().map_err(|e| invalid(format!("step {i}: {e}")))?;
                    if spec.via == Via::Fle && !output.is_front() {
                        return Err(invalid(format!("step {i}: {output} is not reachable through fle")));
                    }
                    levels.set(output, *level);
                }
                steps.push(Step::new(levels, step.hold_ms));
            }
            Ok(Show { id: spec.id, name: name.to_owned(), via: spec.via, repeat: spec.repeat, steps })
        }
    }
}
