use chrono::{DateTime, Duration, Utc};

use crate::Error;
use crate::Predictor;
use crate::frames::TemeState;
use crate::time::{DateTimeIter, IntervalRange};
use crate::vectors::{Position, Velocity};

pub struct PredictionIter {
    predictor: Predictor,
    dt_iter: DateTimeIter,
}

impl From<sgp4::Prediction> for TemeState {
    fn from(value: sgp4::Prediction) -> Self {
        Self::new(
            // Convert sgp4::Prediction.position units (km) to SI (m)
            Position::from_si(
                value.position[0] * 1e3,
                value.position[1] * 1e3,
                value.position[2] * 1e3,
            ),
            // Convert sgp4::Prediction.velocity units (km/s) to SI (m/s)
            Velocity::from_si(
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
}

impl Iterator for PredictionIter {
    type Item = Result<(DateTime<Utc>, TemeState), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let t = self.dt_iter.next()?;

        match self.predictor.propagate(t) {
            Ok(prediction) => Some(Ok((t, prediction))),
            Err(e) => Some(Err(e)),
        }
    }
}
