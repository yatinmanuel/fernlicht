//! What is known about the lighting modules of F-series BMWs: where they sit
//! and the commands they accept.

mod commands;
pub mod tables;

pub use commands::*;
pub use tables::{FemOutput, Lamp, ParseNameError, RemOutput};
