//! Argument-of-latitude event detection over a time interval.
//!
//! [`ArgLatIter`] scans with a fixed step (60 seconds by default) for the
//! moments the satellite reaches a given argument of latitude `u₀` — its
//! angle from the ascending node, measured along the orbit. With `u₀ = 0`
//! that is the ascending node. Like [`ApsisIter`](crate::ApsisIter), it is a thin
//! wrapper over the generic [`EventIter`](crate::EventIter).

use chrono::{DateTime, Duration, Utc};

use crate::{
    Error, Predictor, Result,
    angle::Radians,
    detect::{
        CrossingDetector, DetectIter, EventFunction, EventIter, FixedStep, MIN_POSITIVE_STEP,
        Sample,
    },
    roots::Refinement,
    time,
};

/// Tuning knobs for [`ArgLatIter`]'s coarse scan.
///
/// Pass a customised value to
/// [`Predictor::arg_lat_iter_with_opts`](crate::Predictor::arg_lat_iter_with_opts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArgLatIterOpts {
    /// Fixed step used to scan for sign changes.
    pub step: Duration,
}

impl Default for ArgLatIterOpts {
    fn default() -> Self {
        Self {
            step: Duration::seconds(60),
        }
    }
}

/// Event function: `sin(u − u₀)`, where `u` is the osculating argument of
/// latitude from the TEME state.
///
/// `Direction::Rising` zero crossings are at `u₀`; `Falling` ones, at
/// `u₀ + 180°`, are ignored.
#[derive(Debug, Clone)]
pub(crate) struct ArgLatFn {
    predictor: Predictor,
    sin_u0: f64,
    cos_u0: f64,
}

impl EventFunction for ArgLatFn {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Option<Sample>> {
        let (sin_u, cos_u) = self.predictor.sin_cos_argument_of_latitude(t)?;
        Ok(Some(Sample {
            time: t,
            value: sin_u * self.cos_u0 - cos_u * self.sin_u0,
            rate: None,
        }))
    }
}

/// Iterator over argument-of-latitude events within a time interval.
///
/// Created by [`Predictor::arg_lat_iter`](crate::Predictor::arg_lat_iter).
/// Scans in fixed steps (60 seconds by default) and refines each crossing
/// with the bracketed hybrid solver. A non-finite argument yields a single
/// [`Error::NonFiniteArgumentOfLatitude`] and ends; an equatorial orbit, which has no
/// ascending node, yields [`Error::NoAscendingNode`] at every sample.
#[derive(Debug, Clone)]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ArgLatIter {
    // `None` once the argument has been rejected.
    inner: Option<EventIter<ArgLatFn, FixedStep>>,
    rejected: Option<f64>,
}

impl ArgLatIter {
    /// Scan `interval` for argument-of-latitude events. Prefer
    /// [`Predictor::arg_lat_iter`](crate::Predictor::arg_lat_iter), which
    /// supplies the defaults.
    pub fn new(
        predictor: Predictor,
        interval: impl time::IntervalRange,
        argument: impl Into<Radians>,
        opts: ArgLatIterOpts,
        refinement: Refinement,
    ) -> Self {
        let u0 = argument.into().to_f64();
        let (sin_u0, cos_u0) = u0.sin_cos();
        if !u0.is_finite() {
            return Self {
                inner: None,
                rejected: Some(u0),
            };
        }
        let detector = CrossingDetector::new(
            ArgLatFn {
                predictor,
                sin_u0,
                cos_u0,
            },
            FixedStep(opts.step.max(MIN_POSITIVE_STEP)),
            refinement,
        )
        .ignore_falling_crossings();
        Self {
            rejected: None,
            inner: Some(DetectIter::new(interval, detector)),
        }
    }
}

impl Iterator for ArgLatIter {
    type Item = Result<DateTime<Utc>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(value) = self.rejected.take() {
            return Some(Err(Error::NonFiniteArgumentOfLatitude(value)));
        }
        let crossing = match self.inner.as_mut()?.next()? {
            Ok(crossing) => crossing,
            Err(e) => return Some(Err(e)),
        };
        tracing::debug!(time = %crossing.time, "argument of latitude reached");
        Some(Ok(crossing.time))
    }
}

impl Predictor {
    /// Osculating argument of latitude at `t`, in `[0, 360°)`: the angle from
    /// the ascending node to the satellite, measured along the orbit.
    ///
    /// The node is poorly conditioned as the inclination approaches zero, so
    /// the value is noisy for near-equatorial orbits.
    ///
    /// # Errors
    /// Propagation errors, and [`Error::NoAscendingNode`] for an equatorial
    /// orbit.
    pub fn argument_of_latitude(&self, t: DateTime<Utc>) -> Result<Radians> {
        let (sin_u, cos_u) = self.sin_cos_argument_of_latitude(t)?;
        let u = Radians(sin_u.atan2(cos_u)).normalized();
        // `rem_euclid` can round a tiny negative angle up to exactly a full turn.
        Ok(if u.0 >= std::f64::consts::TAU {
            Radians(0.0)
        } else {
            u
        })
    }

    fn sin_cos_argument_of_latitude(&self, t: DateTime<Utc>) -> Result<(f64, f64)> {
        let s = self.propagate(t)?;
        let (r, v) = (s.position, s.velocity);
        let h = (
            r.y * v.z - r.z * v.y,
            r.z * v.x - r.x * v.z,
            r.x * v.y - r.y * v.x,
        );
        // Ascending-node direction (ẑ × h) and the in-plane direction 90° ahead
        // of it (h × n), both unnormalised.
        let node_norm = h.0.hypot(h.1);
        let h_norm = node_norm.hypot(h.2);
        if node_norm < 1e-9 * h_norm {
            return Err(Error::NoAscendingNode);
        }
        let n = (-h.1, h.0, 0.0);
        let p = (-h.2 * h.0, -h.2 * h.1, node_norm * node_norm);
        let r_norm = (r.x * r.x + r.y * r.y + r.z * r.z).sqrt();
        let cos_u = (r.x * n.0 + r.y * n.1 + r.z * n.2) / (node_norm * r_norm);
        let sin_u = (r.x * p.0 + r.y * p.1 + r.z * p.2) / (h_norm * node_norm * r_norm);
        Ok((sin_u, cos_u))
    }

    /// Detect the times the satellite reaches `argument` of latitude.
    ///
    /// The argument of latitude is the angle from the ascending node along the
    /// orbit, so `Degrees(0.0)` yields the ascending node and `Degrees(180.0)`
    /// the descending one. It is not a geographic latitude: `Degrees(30.0)`
    /// and `Degrees(210.0)` lie in opposite hemispheres.
    pub fn arg_lat_iter(
        &self,
        interval: impl time::IntervalRange,
        argument: impl Into<Radians>,
    ) -> ArgLatIter {
        self.arg_lat_iter_with_opts(
            interval,
            argument,
            ArgLatIterOpts::default(),
            self.refinement,
        )
    }

    /// Like [`Predictor::arg_lat_iter`], but with a customized root-finder
    /// configuration and coarse-scan tuning. See [`Refinement`] and
    /// [`ArgLatIterOpts`].
    pub fn arg_lat_iter_with_opts(
        &self,
        interval: impl time::IntervalRange,
        argument: impl Into<Radians>,
        opts: ArgLatIterOpts,
        refinement: Refinement,
    ) -> ArgLatIter {
        ArgLatIter::new(self.clone(), interval, argument, opts, refinement)
    }
}
