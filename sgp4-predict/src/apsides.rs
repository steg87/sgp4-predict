//! Apsis (apogee and perigee) detection over a time interval.
//!
//! [`ApsisIter`] scans with a fixed step (60 seconds by default), monitoring
//! the sign of the radial velocity `r·v`. A sign change brackets an apsis
//! event, which is then refined with the bracketed hybrid solver. It is a
//! thin wrapper over the generic [`EventIter`](crate::EventIter): `r·v` is
//! the event function and its zero crossings are the apsides.

use chrono::{DateTime, Duration, Utc};

use crate::{
    Predictor, Result,
    detect::{
        CrossingDetector, DetectIter, Direction, EventFunction, EventIter, FixedStep,
        MIN_POSITIVE_STEP, Sample,
    },
    frames::WGS84_A,
    roots::Refinement,
    time,
};

/// Tuning knobs for [`ApsisIter`]'s coarse scan.
///
/// The default reproduces the fixed behavior `ApsisIter` used before this was
/// configurable: a 60-second fixed step. Pass a customized value to
/// [`Predictor::apsis_iter_with_opts`](crate::Predictor::apsis_iter_with_opts).
#[derive(Debug, Clone, Copy)]
pub struct ApsisIterOpts {
    /// Fixed step used to scan for `r·v` sign changes.
    pub step: Duration,
}

impl Default for ApsisIterOpts {
    fn default() -> Self {
        Self {
            step: Duration::seconds(60),
        }
    }
}

/// The type of an apsis event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApsisEvent {
    /// Point of closest approach to Earth (minimum altitude).
    Perigee,
    /// Point of greatest distance from Earth (maximum altitude).
    Apogee,
}

/// A detected apsis event with refined time and altitude.
#[derive(Debug, Clone)]
pub struct Apsis {
    /// Time of the apsis.
    pub time: DateTime<Utc>,
    /// Whether this is an apogee or perigee.
    pub event: ApsisEvent,
    /// Altitude above the WGS-84 equatorial radius in metres.
    pub altitude: f64,
}

/// Event function: the satellite's radial velocity `r·v` in the TEME frame.
///
/// Zero crossings are apsides: falling (positive → negative) at apogee,
/// rising (negative → positive) at perigee.
pub(crate) struct RadialVelocity {
    predictor: Predictor,
}

impl EventFunction for RadialVelocity {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        Ok(Sample {
            time: t,
            value: self.predictor.propagate(t)?.radial_velocity(),
            rate: None,
        })
    }
}

/// Iterator over apogee and perigee events within a time interval.
///
/// Created by [`Predictor::apsis_iter`](crate::Predictor::apsis_iter).
/// Scans in fixed steps (60 seconds by default) and refines each crossing
/// with the bracketed hybrid solver.
pub struct ApsisIter {
    predictor: Predictor,
    inner: EventIter<RadialVelocity, FixedStep>,
}

impl ApsisIter {
    pub fn new(
        predictor: Predictor,
        interval: impl time::IntervalRange,
        opts: ApsisIterOpts,
        refinement: Refinement,
    ) -> Self {
        let detector = CrossingDetector::new(
            RadialVelocity {
                predictor: predictor.clone(),
            },
            FixedStep(opts.step.max(MIN_POSITIVE_STEP)),
            refinement,
        );
        Self {
            predictor,
            inner: DetectIter::new(interval, detector),
        }
    }
}

impl Iterator for ApsisIter {
    type Item = Result<Apsis>;

    fn next(&mut self) -> Option<Self::Item> {
        let crossing = match self.inner.next()? {
            Ok(crossing) => crossing,
            Err(e) => return Some(Err(e)),
        };

        let event = match crossing.direction {
            Direction::Falling => ApsisEvent::Apogee, // r·v went positive → negative: apogee
            Direction::Rising => ApsisEvent::Perigee, // r·v went negative → positive: perigee
        };

        let state = match self.predictor.propagate(crossing.time) {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        let p = state.position;
        let altitude = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt() - WGS84_A;

        let apsis = Apsis {
            time: crossing.time,
            event,
            altitude,
        };
        tracing::debug!(
            event = ?apsis.event,
            time = %apsis.time,
            altitude_km = apsis.altitude / 1_000.0,
            "apsis detected"
        );
        Some(Ok(apsis))
    }
}

impl Predictor {
    /// Detect apogee and perigee events over a time interval.
    ///
    /// Returns an iterator over apsis events in the TEME frame.
    pub fn apsis_iter(&self, interval: impl time::IntervalRange) -> ApsisIter {
        self.apsis_iter_with_opts(interval, ApsisIterOpts::default(), self.refinement)
    }

    /// Like [`Predictor::apsis_iter`], but with a customized root-finder
    /// configuration and coarse-scan tuning. See [`Refinement`] and
    /// [`ApsisIterOpts`].
    pub fn apsis_iter_with_opts(
        &self,
        interval: impl time::IntervalRange,
        opts: ApsisIterOpts,
        refinement: Refinement,
    ) -> ApsisIter {
        ApsisIter::new(self.clone(), interval, opts, refinement)
    }
}
