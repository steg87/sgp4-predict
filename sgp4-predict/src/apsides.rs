//! Apsis (apogee and perigee) detection over a time interval.
//!
//! [`ApsisIter`] scans with a fixed 60-second step, monitoring the sign of
//! the radial velocity `r·v`. A sign change brackets an apsis event, which
//! is then refined with the bracketed hybrid solver. It is a thin wrapper
//! over the generic [`EventIter`](crate::EventIter): `r·v` is the event
//! function and its zero crossings are the apsides.

use chrono::{DateTime, Duration, Utc};

use crate::{
    Predictor, Result,
    detect::{
        CrossingDetector, DetectIter, Direction, EventFunction, EventIter, FixedStep, Sample,
    },
    frames::WGS84_A,
    roots::Refinement,
    time,
};

const STEP: Duration = Duration::seconds(60);

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
/// Scans in 60-second steps and refines each crossing with the bracketed
/// hybrid solver.
pub struct ApsisIter {
    predictor: Predictor,
    inner: EventIter<RadialVelocity, FixedStep>,
}

impl ApsisIter {
    pub fn new(predictor: Predictor, interval: impl time::IntervalRange) -> Self {
        let detector = CrossingDetector::new(
            RadialVelocity {
                predictor: predictor.clone(),
            },
            FixedStep(STEP),
            Refinement::default(),
        );
        Self {
            predictor,
            inner: DetectIter::new(interval, detector),
        }
    }

    /// Override the root-finder configuration used to refine apsis crossings.
    pub fn with_refinement(mut self, r: Refinement) -> Self {
        *self.inner.detector_mut().refinement_mut() = r;
        self
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
        ApsisIter::new(self.clone(), interval).with_refinement(self.refinement)
    }
}
