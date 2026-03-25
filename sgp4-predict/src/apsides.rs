//! Apsis (apogee and perigee) detection over a time interval.
//!
//! [`ApsisIter`] scans with a fixed 60-second step, monitoring the sign of
//! the radial velocity `r·v`. A sign change brackets an apsis event, which
//! is then refined with Brent's method.

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, Result, roots, time};
use roots::Brent;

const STEP: Duration = Duration::seconds(60);
const WGS84_A: f64 = 6_378_137.0; // WGS-84 equatorial radius, metres

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

/// Iterator over apogee and perigee events within a time interval.
///
/// Created by [`Predictor::apsis_iter`](crate::Predictor::apsis_iter).
/// Scans in 60-second steps and refines each crossing with Brent's method.
pub struct ApsisIter {
    predictor: Predictor,
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    prev: Option<(f64, f64)>, // (timestamp as f64, r·v)
    brent: Brent,
}

impl ApsisIter {
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

    fn radial_velocity_at(&self, t: DateTime<Utc>) -> Result<f64> {
        Ok(self.predictor.propagate(t)?.radial_velocity())
    }
}

impl Iterator for ApsisIter {
    type Item = Result<Apsis>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            let t = self.next_time;
            let t_f64 = time::datetime_to_f64(t);

            let rv = match self.radial_velocity_at(t) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            if let Some((prev_t, prev_rv)) = self.prev
                && prev_rv * rv < 0.0
            {
                // Sign change detected — bracket is [prev_t, t_f64]
                let predictor = self.predictor.clone();
                match self.brent.solve(prev_t, t_f64, |x| {
                    let t = time::f64_to_datetime(x);
                    predictor.propagate(t).map(|s| s.radial_velocity())
                }) {
                    Ok(t_refined) => {
                        self.prev = Some((t_f64, rv));
                        self.next_time += STEP;

                        let refined_dt = time::f64_to_datetime(t_refined);
                        let state = match self.predictor.propagate(refined_dt) {
                            Ok(s) => s,
                            Err(e) => return Some(Err(e)),
                        };
                        let p = state.position;
                        let altitude = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt() - WGS84_A;

                        let event = if prev_rv > 0.0 {
                            ApsisEvent::Apogee // r·v went positive → negative: apogee
                        } else {
                            ApsisEvent::Perigee // r·v went negative → positive: perigee
                        };

                        let apsis = Apsis {
                            time: refined_dt,
                            event,
                            altitude,
                        };
                        tracing::debug!(
                            event = ?apsis.event,
                            time = %apsis.time,
                            altitude_km = apsis.altitude / 1_000.0,
                            "apsis detected"
                        );
                        return Some(Ok(apsis));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Brent solver failed to refine apsis crossing");
                        self.prev = Some((t_f64, rv));
                        self.next_time += STEP;
                        return Some(Err(Error::Roots(e)));
                    }
                };
            }

            self.prev = Some((t_f64, rv));
            self.next_time += STEP;
        }
        None
    }
}
