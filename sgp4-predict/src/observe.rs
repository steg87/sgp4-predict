//! Observer trait and ground-based observation types.
//!
//! [`Observer`] represents a fixed point on Earth's surface. Implement it on
//! your own type to use [`Predictor::observe_at`] and the observation iterators.
//!
//! [`Predictor::observe_at`]: crate::Predictor::observe_at

use chrono::{DateTime, Duration, Utc};
use std::fmt;

use crate::{
    Predictor, Result,
    angle::{Degrees, Radians},
    frames::EcefState,
    predict::PredictionIter,
    time::IntervalRange,
};

/// A fixed point on Earth's surface from which satellite passes are observed.
///
/// Altitude is in **metres** above the WGS-84 ellipsoid.
pub trait Observer {
    /// Geodetic latitude (positive north).
    fn latitude(&self) -> Degrees;
    /// Geodetic longitude (positive east).
    fn longitude(&self) -> Degrees;
    /// Height above the WGS-84 ellipsoid in metres.
    fn altitude(&self) -> f64;
}

pub(crate) trait ObserverExt: Observer {
    fn to_ecef(&self) -> EcefState {
        crate::frames::ecef_from_geodetic(self.latitude(), self.longitude(), self.altitude())
    }
}

impl<T: Observer> ObserverExt for T {}

/// A point observation of a satellite from a ground location.
///
/// Range is in **metres**, range rate in **metres per second**. Use
/// `.to_degrees()` on [`azimuth`](Observation::azimuth) or
/// [`elevation`](Observation::elevation) for degree equivalents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    /// Azimuth from north, measured clockwise, in `(-π, π]`. Call
    /// [`normalized`](Radians::normalized) for the `[0, 2π)` convention.
    pub azimuth: Radians,
    /// Elevation above the horizon.
    pub elevation: Radians,
    /// Slant range from observer to satellite in metres.
    pub range: f64,
    /// Rate of change of slant range in metres per second (positive = receding).
    pub range_rate: f64,
}

/// Iterator over time-stamped [`Observation`]s at regular intervals.
///
/// Created by [`Predictor::observation_iter`](crate::Predictor::observation_iter).
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ObservationIter<'a, O: Observer> {
    predict_iter: PredictionIter,
    observer: &'a O,
}

// `O` is only ever held behind a shared reference, so a derive's `O: Debug` /
// `O: Clone` bounds would be a false requirement on caller-supplied observers.
impl<O: Observer> fmt::Debug for ObservationIter<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservationIter")
            .field("predict_iter", &self.predict_iter)
            .finish_non_exhaustive()
    }
}

impl<O: Observer> Clone for ObservationIter<'_, O> {
    fn clone(&self) -> Self {
        Self {
            predict_iter: self.predict_iter.clone(),
            observer: self.observer,
        }
    }
}

impl<'a, O: Observer> ObservationIter<'a, O> {
    /// Sample observations across `interval` every `step`. Prefer
    /// [`Predictor::observation_iter`](crate::Predictor::observation_iter).
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
    type Item = Result<(DateTime<Utc>, Observation)>;

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

impl Predictor {
    /// Observe the satellite from `observer` at time `t`.
    ///
    /// Returns its azimuth, elevation, range and range rate as seen from there.
    pub fn observe_at<O: Observer>(&self, t: DateTime<Utc>, observer: &O) -> Result<Observation> {
        let observation = self
            .propagate(t)?
            .to_ecef(t)
            .to_enu(observer)
            .to_observation();
        Ok(observation)
    }

    /// Observe the satellite from `observer` across a time interval.
    ///
    /// Returns an iterator over time-stamped observations, one every `step`.
    pub fn observation_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        step: Duration,
    ) -> ObservationIter<'a, O> {
        ObservationIter::new(self.clone(), observer, interval, step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{angle::Degrees, types::GroundObserver};

    #[test]
    fn test_to_ecef_equator_prime_meridian() {
        // At lat=0°, lon=0°, alt=0 the ECEF position is exactly [a, 0, 0]
        // where a = 6 378 137 m (WGS-84 semi-major axis).
        let obs = GroundObserver::new(Degrees(0.0), Degrees(0.0), 0.0);
        let ecef = obs.to_ecef();
        assert!((ecef.position.x - 6_378_137.0).abs() < 1.0);
        assert!(ecef.position.y.abs() < 1e-6);
        assert!(ecef.position.z.abs() < 1e-6);
    }

    #[test]
    fn test_to_ecef_north_pole() {
        // At the geographic north pole the ECEF position is [0, 0, b]
        // where b ≈ 6 356 752.314 m (WGS-84 semi-minor axis).
        let obs = GroundObserver::new(Degrees(90.0), Degrees(0.0), 0.0);
        let ecef = obs.to_ecef();
        assert!(ecef.position.x.abs() < 1.0);
        assert!(ecef.position.y.abs() < 1e-6);
        assert!(
            (ecef.position.z - 6_356_752.314).abs() < 1.0,
            "north-pole z = {:.3}, expected ≈ 6 356 752.314",
            ecef.position.z
        );
    }

    #[test]
    fn test_to_ecef_velocity_is_zero() {
        // A stationary ground observer has no velocity in ECEF.
        let obs = GroundObserver::new(Degrees(28.6), Degrees(77.2), 100.0);
        let ecef = obs.to_ecef();
        assert_eq!(ecef.velocity, crate::vectors::Velocity::new(0.0, 0.0, 0.0));
    }
}
