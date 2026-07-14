//! Pole-approach (northernmost and southernmost point) detection over a time interval.
//!
//! [`PoleApproachIter`] scans with a fixed 60-second step, monitoring the sign of
//! the TEME z-velocity component. A sign change brackets a pole-approach event,
//! which is then refined with Brent's method.
//!
//! The z-component of position (and therefore of velocity) is invariant under
//! the TEME→ECEF conversion, since that conversion is a rotation about the
//! Z-axis (see [`TemeState::to_ecef`](crate::TemeState::to_ecef)). This means
//! the extrema of z — and of the geocentric latitude `asin(z / |r|)` derived
//! from it — can be found directly from the TEME state, with no frame
//! conversion needed.

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, Result, roots, time};
use roots::Brent;

const STEP: Duration = Duration::seconds(60);

/// The type of a pole-approach event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoleEvent {
    /// Closest approach to the North Pole (maximum latitude).
    North,
    /// Closest approach to the South Pole (minimum latitude).
    South,
}

/// A detected pole-approach event with refined time and latitude.
#[derive(Debug, Clone)]
pub struct PoleApproach {
    /// Time of closest approach.
    pub time: DateTime<Utc>,
    /// Whether this is a northern or southern approach.
    pub event: PoleEvent,
    /// Geocentric latitude in radians (`asin(z / |r|)`), positive north.
    pub latitude: f64,
}

impl PoleApproach {
    /// Geocentric latitude in degrees, positive north.
    pub fn latitude_deg(&self) -> f64 {
        self.latitude.to_degrees()
    }
}

/// Iterator over northernmost and southernmost points within a time interval.
///
/// Created by [`Predictor::pole_approach_iter`](crate::Predictor::pole_approach_iter).
/// Scans in 60-second steps and refines each crossing with Brent's method.
pub struct PoleApproachIter {
    predictor: Predictor,
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    prev: Option<(f64, f64)>, // (timestamp as f64, vz)
    brent: Brent,
}

impl PoleApproachIter {
    pub fn new(predictor: Predictor, interval: impl time::IntervalRange) -> Self {
        Self {
            predictor,
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            prev: None,
            brent: Brent::default(),
        }
    }

    pub fn with_brent(mut self, b: Brent) -> Self {
        self.brent = b;
        self
    }

    fn z_velocity_at(&self, t: DateTime<Utc>) -> Result<f64> {
        Ok(self.predictor.propagate(t)?.velocity.z)
    }
}

impl Iterator for PoleApproachIter {
    type Item = Result<PoleApproach>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            let t = self.next_time;
            let t_f64 = time::datetime_to_f64(t);

            let vz = match self.z_velocity_at(t) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            if let Some((prev_t, prev_vz)) = self.prev
                && prev_vz * vz < 0.0
            {
                // Sign change detected — bracket is [prev_t, t_f64]
                let predictor = self.predictor.clone();
                match self.brent.solve(prev_t, t_f64, |x| {
                    let t = time::f64_to_datetime(x);
                    predictor.propagate(t).map(|s| s.velocity.z)
                }) {
                    Ok(t_refined) => {
                        self.prev = Some((t_f64, vz));
                        self.next_time += STEP;

                        let refined_dt = time::f64_to_datetime(t_refined);
                        let state = match self.predictor.propagate(refined_dt) {
                            Ok(s) => s,
                            Err(e) => return Some(Err(e)),
                        };
                        let p = state.position;
                        let r = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt();
                        let latitude = (p.z / r).asin();

                        let event = if prev_vz > 0.0 {
                            PoleEvent::North // vz went positive → negative: max z (north)
                        } else {
                            PoleEvent::South // vz went negative → positive: min z (south)
                        };

                        let approach = PoleApproach {
                            time: refined_dt,
                            event,
                            latitude,
                        };
                        tracing::debug!(
                            event = ?approach.event,
                            time = %approach.time,
                            latitude_deg = approach.latitude_deg(),
                            "pole approach detected"
                        );
                        return Some(Ok(approach));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Brent solver failed to refine pole-approach crossing");
                        self.prev = Some((t_f64, vz));
                        self.next_time += STEP;
                        return Some(Err(Error::Roots(e)));
                    }
                };
            }

            self.prev = Some((t_f64, vz));
            self.next_time += STEP;
        }
        None
    }
}
