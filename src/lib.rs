mod frames;
mod observe;
mod predict;
mod time;
mod transits;
mod units;
mod vectors;

use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch};

pub use crate::frames::TemeState;
pub use crate::observe::{Observation, ObservationIter, Observer};
pub use crate::predict::PredictionIter;
pub use crate::time::IntervalRange;
pub use crate::transits::{Transit, TransitIter};
pub use crate::vectors::{Position, StateVector, Velocity};

pub mod test_utils {
    pub use crate::units::SI;
}

pub trait Satellite: HasId + HasTle {}
impl<T> Satellite for T where T: HasId + HasTle {}

pub trait HasId {
    fn id(&self) -> String;
}

pub trait HasTle {
    fn line_1(&self) -> String;
    fn line_2(&self) -> String;
}

/// Stores orbital elements and constants. Has methods to create iterators to propagate predictions
/// in given frames.
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
}

impl Predictor {
    pub fn new(sat: &impl Satellite) -> Self {
        let elements = Elements::from_tle(
            Some(sat.id()),
            sat.line_1().as_bytes(),
            sat.line_2().as_bytes(),
        )
        .expect("Failed to generate elements for sat");
        let constants =
            Constants::from_elements(&elements).expect("Failed to generate constants for sat");
        Self {
            elements,
            constants,
        }
    }

    /// Propagate the TLE to given time t.
    ///
    /// Returns a predicted state vector in the TEME frame.
    pub fn propagate(&self, t: DateTime<Utc>) -> Result<TemeState, Error> {
        let minutes_since_epoch =
            MinutesSinceEpoch(self.time_since_epoch(t).num_milliseconds() as f64 / 60e3);
        let prediction = self
            .constants
            .propagate(minutes_since_epoch)
            .map_err(Error::Sgp4)?;
        Ok(prediction.into())
    }

    /// Propagate the TLE over a time interval.
    ///
    /// Returns an iterator over predicted state vectors in the TEME frame.
    pub fn prediction_iter(&self, interval: impl IntervalRange, step: Duration) -> PredictionIter {
        PredictionIter::new(self.clone(), interval, step)
    }

    /// Observe the TLE from an observer on Earth.
    ///
    /// Returns an iterator over observations.
    pub fn observation_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        step: Duration,
    ) -> ObservationIter<'a, O> {
        ObservationIter::new(self.clone(), observer, interval, step)
    }

    /// Calculate all of the transits visible to the observer.
    ///
    /// Returns an iterator over transits.
    pub fn transits_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: units::Angle,
    ) -> TransitIter<'a, O> {
        TransitIter::new(self.clone(), observer, interval, min_elevation)
    }

    /// Calculate the number of minutes since the predictor epoch
    pub fn time_since_epoch(&self, t: DateTime<Utc>) -> Duration {
        let epoch = DateTime::<Utc>::from_naive_utc_and_offset(self.elements.datetime, Utc);
        t.signed_duration_since(epoch)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SGP4 error: {0}")]
    Sgp4(sgp4::Error),
    #[error("Interval error: {0}")]
    Interval(String),
}
