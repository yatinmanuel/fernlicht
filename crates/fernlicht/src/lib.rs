//! Drive the exterior lights of an F-series BMW over an ENET cable.
//!
//! The crate is layered, and each layer is usable on its own:
//!
//! - [`uds`] builds diagnostic requests and pairs replies with them.
//! - [`transport`] frames them as HSFZ or DoIP and runs them over TCP, or over
//!   any byte stream that implements [`Transport`](transport::Transport).
//! - [`bmw`] knows the lighting modules: the three light commands, how to
//!   identify the modules by name, and a [`Guard`](bmw::Guard) that lets only
//!   those commands through.
//! - [`show`] turns timed steps into light commands.
//!
//! ```no_run
//! use fernlicht::bmw::{self, Guard, ScanOptions};
//! use fernlicht::show::{self, Player};
//! use fernlicht::transport::{self, ClientOptions};
//!
//! # fn main() -> fernlicht::Result<()> {
//! let client = transport::connect("169.254.92.38", &transport::DEFAULT_ORDER, &ClientOptions::default())?;
//! let report = bmw::identify(&client, ScanOptions::default(), |_| {});
//! let link = Guard::new(&client, report.profile);
//!
//! let welcome = show::find("welcome").expect("built-in show");
//! let mut driver = show::driver_for(&welcome, &link, &report.profile)?;
//! Player::new().play(&welcome, &mut driver)?;
//! # Ok(())
//! # }
//! ```

pub mod bmw;
pub mod bytes;
mod error;
pub mod show;
#[cfg(feature = "serde")]
mod time;
pub mod transport;
pub mod uds;

pub use error::{Error, Result};
