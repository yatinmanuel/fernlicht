//! What is known about the lighting modules of F-series BMWs: where they sit,
//! the commands they accept, and how to recognise them.

mod commands;
mod guard;
mod identify;
pub mod tables;

pub use commands::*;
pub use guard::Guard;
pub use identify::{
    Answer, LightProbe, Module, Profile, Report, ScanOptions, identify, mask_vin, parse_vin, probe_lights,
    read_vin, read_voltage, resolve_profile, scan,
};
pub use tables::{FemOutput, Lamp, ParseNameError, RemOutput};
