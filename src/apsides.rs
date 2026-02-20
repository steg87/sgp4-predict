use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, Result, roots, time};

const STEP: Duration = Duration::seconds(60);
const TOL: f64 = 1e-3;
const WGS84_A: f64 = 6_378_137.0; // WGS-84 equatorial radius, metres

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApsisEvent {
    Perigee,
    Apogee,
}

#[derive(Debug, Clone)]
pub struct Apsis {
    pub time: DateTime<Utc>,
    pub event: ApsisEvent,
    pub altitude: f64, // metres above WGS-84 equatorial radius
}

pub struct ApsisIter {
    predictor: Predictor,
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    prev: Option<(f64, f64)>, // (timestamp as f64, r·v)
}

impl ApsisIter {
    pub fn new(predictor: Predictor, interval: impl time::IntervalRange) -> Self {
        Self {
            predictor,
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            prev: None,
        }
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
                match roots::brent(
                    prev_t,
                    t_f64,
                    |x| {
                        let t = time::f64_to_datetime(x);
                        predictor.propagate(t).map(|s| s.radial_velocity())
                    },
                    TOL,
                    50,
                ) {
                    Ok(t_refined) => {
                        self.prev = Some((t_f64, rv));
                        self.next_time += STEP;

                        let refined_dt = time::f64_to_datetime(t_refined);
                        let state = match self.predictor.propagate(refined_dt) {
                            Ok(s) => s,
                            Err(e) => return Some(Err(e)),
                        };
                        let p = state.position;
                        let altitude =
                            (p.x * p.x + p.y * p.y + p.z * p.z).sqrt() - WGS84_A;

                        let event = if prev_rv > 0.0 {
                            ApsisEvent::Apogee // r·v went positive → negative: apogee
                        } else {
                            ApsisEvent::Perigee // r·v went negative → positive: perigee
                        };

                        return Some(Ok(Apsis {
                            time: refined_dt,
                            event,
                            altitude,
                        }));
                    }
                    Err(e) => {
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
