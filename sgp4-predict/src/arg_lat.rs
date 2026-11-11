//! Argument-of-latitude event detection over a time interval.
//!
//! [`ArgLatIter`] scans with a fixed step (60 seconds by default) for the
//! moments the satellite reaches a given argument of latitude `u₀` — its
//! angle from the ascending node, measured along the orbit — and the
//! opposite point `u₀ + 180°`. With `u₀ = 0` those are the ascending and
//! descending nodes. Like [`ApsisIter`](crate::ApsisIter), it is a thin
//! wrapper over the generic [`EventIter`](crate::EventIter).

use chrono::{DateTime, Duration, Utc};

use crate::{
    Error, Predictor, Result,
    angle::Radians,
    detect::{
        CrossingDetector, DetectIter, Direction, EventFunction, EventIter, FixedStep,
        MIN_POSITIVE_STEP, Sample,
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

/// Which of the two argument-of-latitude points an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgLatEvent {
    /// The satellite reaches the requested argument of latitude `u₀`.
    Rising,
    /// The satellite reaches the opposite point, `u₀ + 180°`.
    Falling,
}

/// A detected argument-of-latitude event with its refined time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArgLatCrossing {
    /// Time of the event.
    pub time: DateTime<Utc>,
    /// Whether this is `u₀` or `u₀ + 180°`.
    pub event: ArgLatEvent,
}

/// Event function: `sin(u − u₀)`, where `u` is the osculating argument of
/// latitude from the TEME state.
///
/// Rising zero crossings are at `u₀`, falling ones at `u₀ + 180°`.
#[derive(Debug, Clone)]
pub(crate) struct ArgLatFn {
    predictor: Predictor,
    sin_u0: f64,
    cos_u0: f64,
}

impl EventFunction for ArgLatFn {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        if !self.sin_u0.is_finite() || !self.cos_u0.is_finite() {
            return Err(Error::Custom("argument of latitude is not finite".into()));
        }
        let s = self.predictor.propagate(t)?;
        let (r, v) = (s.position, s.velocity);
        let h = (
            r.y * v.z - r.z * v.y,
            r.z * v.x - r.x * v.z,
            r.x * v.y - r.y * v.x,
        );
        // Ascending-node direction (ẑ × h) and the in-plane direction 90° ahead
        // of it (h × n), both unnormalised.
        let node_norm = h.0.hypot(h.1);
        if node_norm < 1e-9 * (node_norm.hypot(h.2)) {
            return Err(Error::Custom(
                "equatorial orbit has no ascending node".into(),
            ));
        }
        let n = (-h.1, h.0, 0.0);
        let p = (-h.2 * h.0, -h.2 * h.1, node_norm * node_norm);
        let h_norm = node_norm.hypot(h.2);
        let cos_u = (r.x * n.0 + r.y * n.1 + r.z * n.2) / node_norm;
        let sin_u = (r.x * p.0 + r.y * p.1 + r.z * p.2) / (h_norm * node_norm);
        Ok(Sample {
            time: t,
            value: sin_u * self.cos_u0 - cos_u * self.sin_u0,
            rate: None,
        })
    }
}

/// Iterator over argument-of-latitude events within a time interval.
///
/// Created by [`Predictor::arg_lat_iter`](crate::Predictor::arg_lat_iter).
/// Scans in fixed steps (60 seconds by default) and refines each crossing
/// with the bracketed hybrid solver. Yields an error on every sample for a
/// non-finite argument or an equatorial orbit, which has no ascending node.
#[derive(Debug, Clone)]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct ArgLatIter {
    inner: EventIter<ArgLatFn, FixedStep>,
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
        let (sin_u0, cos_u0) = argument.into().to_f64().sin_cos();
        let detector = CrossingDetector::new(
            ArgLatFn {
                predictor,
                sin_u0,
                cos_u0,
            },
            FixedStep(opts.step.max(MIN_POSITIVE_STEP)),
            refinement,
        );
        Self {
            inner: DetectIter::new(interval, detector),
        }
    }
}

impl Iterator for ArgLatIter {
    type Item = Result<ArgLatCrossing>;

    fn next(&mut self) -> Option<Self::Item> {
        let crossing = match self.inner.next()? {
            Ok(crossing) => crossing,
            Err(e) => return Some(Err(e)),
        };
        let event = match crossing.direction {
            Direction::Rising => ArgLatEvent::Rising,
            Direction::Falling => ArgLatEvent::Falling,
        };
        tracing::debug!(?event, time = %crossing.time, "argument of latitude reached");
        Some(Ok(ArgLatCrossing {
            time: crossing.time,
            event,
        }))
    }
}

impl Predictor {
    /// Detect the times the satellite reaches `argument` of latitude, and the
    /// opposite point 180° round the orbit.
    ///
    /// The argument of latitude is the angle from the ascending node along the
    /// orbit, so `Degrees(0.0)` yields the ascending ([`ArgLatEvent::Rising`])
    /// and descending ([`ArgLatEvent::Falling`]) nodes. It is not a geographic
    /// latitude: `Degrees(30.0)` yields `u = 30°` and `u = 210°`, which lie in
    /// opposite hemispheres.
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
