//! What one end of a satellite-to-ground link sees of the other.
//!
//! [`Observation`] is the ground's view of the satellite — azimuth, elevation,
//! range and range rate. [`Pointing`] is the satellite's view of the ground —
//! a direction in the spacecraft's [`LvlhState`] frame, plus the same range and
//! range rate. Anything convertible into a [`GeodeticPoint`] serves as the
//! ground end of either.
//!
//! [`LvlhState`]: crate::LvlhState

use chrono::{DateTime, Duration, Utc};

use crate::{
    Predictor, Result,
    angle::Radians,
    frames::{GeodeticPoint, LvlhDirection},
    predict::PredictionIter,
    time::IntervalRange,
};

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
#[derive(Debug, Clone)]
pub struct ObservationIter {
    predict_iter: PredictionIter,
    observer: GeodeticPoint,
}

impl ObservationIter {
    /// Sample observations across `interval` every `step`. Prefer
    /// [`Predictor::observation_iter`](crate::Predictor::observation_iter).
    pub fn new(
        predictor: Predictor,
        observer: impl Into<GeodeticPoint>,
        interval: impl IntervalRange,
        step: Duration,
    ) -> Self {
        Self {
            predict_iter: PredictionIter::new(predictor, interval, step),
            observer: observer.into(),
        }
    }

    /// Include the interval end time as an extra sample after the last regular step.
    pub fn include_end(mut self) -> Self {
        self.predict_iter = self.predict_iter.include_end();
        self
    }
}

impl Iterator for ObservationIter {
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
    pub fn observe_at(
        &self,
        t: DateTime<Utc>,
        observer: impl Into<GeodeticPoint>,
    ) -> Result<Observation> {
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
    pub fn observation_iter(
        &self,
        observer: impl Into<GeodeticPoint>,
        interval: impl IntervalRange,
        step: Duration,
    ) -> ObservationIter {
        ObservationIter::new(self.clone(), observer, interval, step)
    }
}

/// The satellite's view of a target on the ground.
///
/// `direction` is the primitive: a unit vector in the satellite's
/// [`LvlhState`](crate::LvlhState) frame, which composes directly with an
/// antenna or instrument mounting rotation. Nadir-referenced angles are
/// derived from it — see [`off_nadir`](Pointing::off_nadir) — rather than
/// stored, because a boresight is not always nadir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointing {
    /// Unit vector from the satellite to the target, in LVLH.
    ///
    /// Zero — not a unit vector — in the one degenerate case, a target at the
    /// satellite's own position, where there is no direction to report and
    /// [`range`](Pointing::range) is zero too. Check `range` before composing
    /// this with a mounting rotation if that case is reachable for you.
    pub direction: LvlhDirection,
    /// Slant range from satellite to target in metres.
    pub range: f64,
    /// Rate of change of slant range in metres per second (positive = receding).
    pub range_rate: f64,
}

impl Pointing {
    /// Angle between [`direction`](Pointing::direction) and nadir, in `[0, π]`.
    ///
    /// Zero for the degenerate zero-range state, which is indistinguishable
    /// from a target exactly at nadir — check
    /// [`range`](Pointing::range) to tell them apart.
    ///
    /// Nadir is geocentric — measured from the position vector rather than the
    /// ellipsoid normal — the same convention as
    /// [`AoiIterOpts::max_off_nadir`](crate::AoiIterOpts::max_off_nadir), so
    /// the two compare directly.
    ///
    /// Against an [`Observation`] of the same target, `sin(off_nadir)` equals
    /// `(rₑ/r)·cos(elevation)` exactly where the ellipsoid normal is radial,
    /// and differs by the deflection of the vertical elsewhere — up to about
    /// 0.19° of tilt at mid-latitudes, since `elevation` is measured from the
    /// geodetic horizon.
    #[must_use]
    pub fn off_nadir(&self) -> Radians {
        let (x, y, z) = (self.direction.x, self.direction.y, self.direction.z);
        Radians(x.hypot(y).atan2(z))
    }
}

impl Predictor {
    /// Point the satellite at `target` at time `t`.
    ///
    /// Returns the direction to the target in the satellite's LVLH frame,
    /// along with slant range and range rate.
    ///
    /// This is pure geometry and does **not** test line of sight: a target on
    /// the far side of the Earth returns normally, with an
    /// [`off_nadir`](Pointing::off_nadir) past the horizon angle. Use
    /// [`observe_at`](Predictor::observe_at) and check its
    /// [`elevation`](Observation::elevation) for visibility.
    ///
    /// ```no_run
    /// # use sgp4_predict::{Degrees, GeodeticPoint, Predictor, Tle};
    /// # use chrono::Utc;
    /// # let tle: Tle = "ISS (ZARYA)\n1 ...\n2 ...".parse().unwrap();
    /// let predictor = Predictor::from_tle(&tle).unwrap();
    /// let t = Utc::now();
    ///
    /// let glasgow = GeodeticPoint {
    ///     latitude: Degrees(55.86),
    ///     longitude: Degrees(-4.25),
    ///     altitude: 40.0,
    /// };
    /// let pointing = predictor.point_at(t, glasgow).unwrap();
    /// println!(
    ///     "{:.1} km away, {:.1}° off nadir",
    ///     pointing.range / 1000.0,
    ///     pointing.off_nadir().degrees(),
    /// );
    /// ```
    pub fn point_at(&self, t: DateTime<Utc>, target: impl Into<GeodeticPoint>) -> Result<Pointing> {
        let satellite = self.propagate(t)?;
        let target = target.into().to_ecef().to_teme(t);
        Ok(satellite.to_lvlh(&target).to_pointing())
    }
}
