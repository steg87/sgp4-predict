//! Observer trait and ground-based observation types.
//!
//! [`Observer`] represents a fixed point on Earth's surface. Implement it on
//! your own type to use [`Predictor::observe_at`] and the observation iterators.
//!
//! [`Predictor::observe_at`]: crate::Predictor::observe_at

use chrono::{DateTime, Duration, Utc};

use crate::{
    Error, Predictor,
    frames::EcefState,
    predict::PredictionIter,
    time::IntervalRange,
    vectors::{Position, Velocity, StateVector},
};

/// A fixed point on Earth's surface from which satellite passes are observed.
///
/// All angular values must be in **radians**. Altitude is in **metres**
/// above the WGS-84 ellipsoid.
pub trait Observer {
    /// Geodetic latitude in radians (positive north).
    fn latitude(&self) -> f64;
    /// Geodetic longitude in radians (positive east).
    fn longitude(&self) -> f64;
    /// Height above the WGS-84 ellipsoid in metres.
    fn altitude(&self) -> f64;
}

pub(crate) trait ObserverExt: Observer {
    fn to_ecef(&self) -> EcefState {
        let h = self.altitude();
        let a = 6378137.0; // meters
        let f = 1.0 / 298.257223563;
        let e2 = f * (2.0 - f);

        let sin_lat = self.latitude().sin();
        let cos_lat = self.latitude().cos();
        let sin_lon = self.longitude().sin();
        let cos_lon = self.longitude().cos();

        let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

        StateVector::new(
            Position::new(
                (n + h) * cos_lat * cos_lon,
                (n + h) * cos_lat * sin_lon,
                (n * (1.0 - e2) + h) * sin_lat,
            ),
            Velocity::default(),
        )
    }
}

impl<T: Observer> ObserverExt for T {}

/// A point observation of a satellite from a ground location.
///
/// Angular values are in **radians**, range in **metres**, range rate in
/// **metres per second**.
#[derive(Debug, Clone)]
pub struct Observation {
    /// Azimuth from north, measured clockwise, in radians.
    pub azimuth: f64,
    /// Elevation above the horizon in radians.
    pub elevation: f64,
    /// Slant range from observer to satellite in metres.
    pub range: f64,
    /// Rate of change of slant range in metres per second (positive = receding).
    pub range_rate: f64,
}

/// Iterator over time-stamped [`Observation`]s at regular intervals.
///
/// Created by [`Predictor::observation_iter`](crate::Predictor::observation_iter).
pub struct ObservationIter<'a, O: Observer> {
    predict_iter: PredictionIter,
    observer: &'a O,
}

impl<'a, O: Observer> ObservationIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl IntervalRange,
        step: Duration,
    ) -> Self {
        Self {
            predict_iter: PredictionIter::new(predictor, interval, step),
            observer,
        }
    }

    /// Include the interval end time as an extra sample after the last regular step.
    pub fn include_end(mut self) -> Self {
        self.predict_iter = self.predict_iter.include_end();
        self
    }
}

impl<'a, O: Observer> Iterator for ObservationIter<'a, O> {
    type Item = Result<(DateTime<Utc>, Observation), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let (time, teme_state) = match self.predict_iter.next()? {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        Some(Ok((
            time,
            teme_state
                .to_ecef(time)
                .to_enu(self.observer)
                .to_observation(),
        )))
    }
}
