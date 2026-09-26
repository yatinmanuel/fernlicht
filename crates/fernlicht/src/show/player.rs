use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use super::{Driver, Lights, Show};
use crate::{Error, Result};

const SPEED_RANGE: (f64, f64) = (0.25, 4.0);

/// Plays shows through a [`Driver`] on the calling thread.
///
/// Stop it from anywhere through a [`StopHandle`]; the show ends after the
/// current step and the lamps are handed back before `play` returns. A stopped
/// player stays stopped.
#[derive(Debug, Default)]
pub struct Player {
    speed: f64,
    stop: StopHandle,
}

impl Player {
    pub fn new() -> Self {
        Self { speed: 1.0, stop: StopHandle::default() }
    }

    /// Playback speed, clamped to 0.25–4.
    #[must_use]
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }

    /// Plays `show` until it ends or is stopped.
    ///
    /// The driver's `release` always runs once `begin` has been called. If it
    /// fails, that error wins over any earlier one, since it means the lamps
    /// may still be forced.
    pub fn play(&self, show: &Show, driver: &mut dyn Driver) -> Result<()> {
        if show.steps.is_empty() || (show.repeat && show.steps.iter().all(|s| s.hold.is_zero())) {
            return Err(Error::InvalidShow(format!("{}: nothing to play", show.id)));
        }
        if self.stop.is_stopped() {
            return Ok(());
        }
        let played = self.run(show, driver);
        match driver.release() {
            Ok(()) => played,
            Err(err) => Err(Error::NotRestored(Box::new(err))),
        }
    }

    fn run(&self, show: &Show, driver: &mut dyn Driver) -> Result<()> {
        let speed = if self.speed.is_finite() { self.speed.clamp(SPEED_RANGE.0, SPEED_RANGE.1) } else { 1.0 };
        let mut lights = Lights::dark();
        driver.begin()?;
        loop {
            for step in &show.steps {
                if self.stop.is_stopped() {
                    return Ok(());
                }
                let hold = step.hold.div_f64(speed);
                lights.apply(&step.levels);
                driver.frame(&lights, hold)?;
                self.stop.sleep(hold);
            }
            if !show.repeat {
                return Ok(());
            }
        }
    }
}

/// Stops a [`Player`] from another thread, a signal handler, or a UI callback.
#[derive(Debug, Clone, Default)]
pub struct StopHandle(Arc<(Mutex<bool>, Condvar)>);

impl StopHandle {
    pub fn stop(&self) {
        let (stopped, wake) = &*self.0;
        *stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sleeps for `duration` or until stopped.
    fn sleep(&self, duration: Duration) {
        let (stopped, wake) = &*self.0;
        let guard = stopped.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = wake.wait_timeout_while(guard, duration, |stopped| !*stopped);
    }
}
