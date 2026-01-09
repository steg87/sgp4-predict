use std::ops::Range;

use chrono::{DateTime, Duration, Utc};

use crate::Error;
use crate::Observation;
use crate::Predictor;
use crate::observe::Observer;
use crate::time::IntervalRange;
use crate::units::{self, SI};

pub struct Transit {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Transit {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }
}

impl IntervalRange for Transit {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

pub struct TransitIter<'a, O: Observer> {
    predictor: Predictor,
    observer: &'a O,
    interval: Range<DateTime<Utc>>,
    min_elevation: units::Angle,
    next_time: DateTime<Utc>,
    state: Option<TransitState>,
}

impl<'a, O: Observer> TransitIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: units::Angle,
    ) -> Self {
        Self {
            predictor,
            observer,
            interval: interval.start()..interval.end(),
            min_elevation,
            next_time: interval.start(),
            state: None,
        }
    }

    /// Takes a new observation and determines if a transit has been entered by comparing the new
    /// observation state with the previous.
    ///
    /// On entering a transit it will calculate the roots (start, end) of the transit and return it.
    fn detect_transit(&mut self, new_state: &mut TransitState) -> Option<Transit> {
        // Define refinement cost function closure
        let f = |t| {
            let t = DateTime::from_timestamp(t as i64, 0).unwrap();
            let el = self
                .predictor
                .observe_at(t, self.observer)
                .unwrap()
                .elevation;
            (el - self.min_elevation).to_si()
        };

        // Determine if state transition indicates that a new transit has been found
        let start = match &self.state {
            // Previous example exists, check if we have transitioned into a transit
            Some(prev_state) => {
                match (prev_state, &*new_state) {
                    (TransitState::Outside(t0), TransitState::Inside(t1, _)) => {
                        // Transitioned into a transit, refine transit start and return
                        let start = refine_crossing(
                            datetime_to_f64(*t0),
                            datetime_to_f64(*t1),
                            f,
                            0.001,
                            1e-6,
                        );
                        DateTime::<Utc>::from_timestamp_nanos((start * 1e9) as i64)
                    }
                    _ => return None, // No other state transitions of interest
                }
            }
            None => {
                match &*new_state {
                    TransitState::Inside(t1, o1) if o1.elevation == self.min_elevation => {
                        // This is an edge case where the first observation is the start of a
                        // transit, i.e. the start of the first transit is exactly concurrent with
                        // the start of iter interval.
                        *t1
                    }
                    _ => return None,
                }
            }
        };

        // new_state must be Inside at this point, advance time until the state is Outside
        let mut t0 = start;
        let mut t1 = t0 + self.step_size();
        let end = loop {
            if (t1 - start) > Duration::hours(1) {
                // TODO: log warning transit was longer than an hour and was ignored
                return None;
            }
            let observation = self.predictor.observe_at(t1, self.observer).unwrap(); // TODO
            if observation.elevation < self.min_elevation {
                // Transitioned out of a transit, refine transit end and return
                let end = refine_crossing(datetime_to_f64(t0), datetime_to_f64(t1), f, 0.001, 1e-6);
                break DateTime::<Utc>::from_timestamp_nanos((end * 1e9) as i64);
            };
            t0 = t1;
            t1 += self.step_size();
        };
        // Update the state and next time so the next iteration picks up from here
        (self.next_time, *new_state) = (t1, TransitState::Outside(t1));
        Some(Transit::new(start, end))
    }

    fn step_size(&self) -> Duration {
        Duration::seconds(15) // TODO
    }
}

impl<'a, O: Observer> Iterator for TransitIter<'a, O> {
    type Item = Result<Transit, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            // Calculate observation at current time
            let t = self.next_time;
            let observation = match self.predictor.observe_at(t, self.observer) {
                Ok(obs) => obs,
                Err(e) => return Some(Err(e)),
            };
            let mut new_state = if observation.elevation >= self.min_elevation {
                TransitState::Inside(t, observation)
            } else {
                TransitState::Outside(t)
            };

            // Detect transit, if any. Calculate step size based on result.
            let result = self.detect_transit(&mut new_state);
            // Calculate next step size
            self.next_time += self.step_size();
            // Update current state
            self.state.replace(new_state);
            // If transit found then return it, otherwise continue
            if let Some(transit) = result {
                return Some(Ok(transit));
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
enum TransitState {
    Inside(DateTime<Utc>, Observation),
    Outside(DateTime<Utc>),
}

fn refine_crossing<F>(mut t0: f64, mut t1: f64, mut f: F, tol_time: f64, tol_val: f64) -> f64
where
    F: FnMut(f64) -> f64,
{
    let f_lo = f(t0);
    let f_hi = f(t1);

    assert!(f_lo != 0.0 && f_hi != 0.0);
    assert!(f_lo.signum() != f_hi.signum());

    // Normalise so f(t0) < 0 and f(t1) > 0
    let flip = if f_lo > f_hi { -1.0 } else { 1.0 };
    let mut f_norm = |t: f64| flip * f(t);

    let mut t = 0.5 * (t0 + t1);

    for _ in 0..20 {
        let v = f_norm(t);
        if v.abs() < tol_val || (t1 - t0).abs() < tol_time {
            return t;
        }

        let h = 1.0;
        let v_plus = f_norm(t + h);
        let v_minus = f_norm(t - h);
        let deriv = (v_plus - v_minus) / (2.0 * h);

        let mut new_t = if deriv.abs() > 1e-12 {
            t - v / deriv
        } else {
            0.5 * (t0 + t1)
        };

        if new_t <= t0 || new_t >= t1 {
            new_t = 0.5 * (t0 + t1);
        }

        if f_norm(new_t) > 0.0 {
            t1 = new_t;
        } else {
            t0 = new_t;
        }

        t = new_t;
    }
    // TODO: log warning max iterations reached
    t
}

fn datetime_to_f64(dt: DateTime<Utc>) -> f64 {
    let secs = dt.timestamp() as f64; // integer seconds since Unix epoch
    let nanos = dt.timestamp_subsec_nanos() as f64; // fractional nanoseconds
    secs + nanos * 1e-9
}
