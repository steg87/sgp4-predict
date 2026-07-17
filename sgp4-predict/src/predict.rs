//! Iterator over propagated TEME state vectors at regular time intervals.

use chrono::{DateTime, Duration, Utc};

use crate::{
    Predictor, Result,
    frames::TemeState,
    time::{DateTimeIter, IntervalRange},
    vectors::{Position, Velocity},
};

/// Iterator over time-stamped TEME state vectors at regular intervals.
///
/// Created by [`Predictor::prediction_iter`](crate::Predictor::prediction_iter).
pub struct PredictionIter {
    predictor: Predictor,
    dt_iter: DateTimeIter,
}

impl From<sgp4::Prediction> for TemeState {
    fn from(value: sgp4::Prediction) -> Self {
        Self::new(
            // Convert sgp4::Prediction.position units (km) to SI (m)
            Position::new(
                value.position[0] * 1e3,
                value.position[1] * 1e3,
                value.position[2] * 1e3,
            ),
            // Convert sgp4::Prediction.velocity units (km/s) to SI (m/s)
            Velocity::new(
                value.velocity[0] * 1e3,
                value.velocity[1] * 1e3,
                value.velocity[2] * 1e3,
            ),
        )
    }
}

impl PredictionIter {
    pub(crate) fn new(predictor: Predictor, interval: impl IntervalRange, step: Duration) -> Self {
        Self {
            predictor,
            dt_iter: DateTimeIter::new(interval, step),
        }
    }

    /// Include the interval end time as an extra sample after the last regular step.
    pub fn include_end(mut self) -> Self {
        self.dt_iter = self.dt_iter.include_end();
        self
    }
}

impl Iterator for PredictionIter {
    type Item = Result<(DateTime<Utc>, TemeState)>;

    fn next(&mut self) -> Option<Self::Item> {
        let t = self.dt_iter.next()?;

        match self.predictor.propagate(t) {
            Ok(prediction) => Some(Ok((t, prediction))),
            Err(e) => Some(Err(e)),
        }
    }
}

impl Predictor {
    /// Propagate the TLE over a time interval.
    ///
    /// Returns an iterator over predicted state vectors in the TEME frame.
    pub fn prediction_iter(&self, interval: impl IntervalRange, step: Duration) -> PredictionIter {
        PredictionIter::new(self.clone(), interval, step)
    }
}
