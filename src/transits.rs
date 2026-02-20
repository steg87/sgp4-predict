use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, Result, observe::Observer, roots, time};

const MAX_STEP: Duration = Duration::minutes(10);
const MIN_STEP: Duration = Duration::seconds(10);

pub struct Transit {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Transit {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }
}

impl time::IntervalRange for Transit {
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
    min_elevation: f64,
    next_time: DateTime<Utc>,
    state: Option<TransitState>,
}

impl<'a, O: Observer> TransitIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl time::IntervalRange,
        min_elevation: f64,
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
    fn detect_transit(&mut self, new_state: &mut TransitState) -> Result<Option<Transit>> {
        let mut f = |t: f64| {
            self.calculate_elevation(time::f64_to_datetime(t))
                .map(|(el, el_rate)| (el - self.min_elevation, el_rate))
        };

        // Determine if state transition indicates that a new transit has been found
        let start = match &self.state {
            // Previous state exists, check if we have transitioned into a transit
            Some(prev_state) => {
                match (prev_state, &*new_state) {
                    (TransitState::Outside(t0), TransitState::Inside(t1, _)) => {
                        // Transitioned into a transit, refine transit start
                        let start = refine_crossing(
                            time::datetime_to_f64(*t0),
                            time::datetime_to_f64(*t1),
                            &mut f,
                        )?;
                        time::f64_to_datetime(start)
                    }
                    _ => return Ok(None), // No other state transitions of interest
                }
            }
            None => {
                match &*new_state {
                    TransitState::Inside(t1, el) if *el == self.min_elevation => {
                        // Edge case: first observation is exactly the start of a transit.
                        *t1
                    }
                    _ => return Ok(None),
                }
            }
        };

        // new_state must be Inside at this point, advance time until the state is Outside
        let mut t0 = start;
        let step = Duration::seconds(30); // Fixed step, el_rate won't help cross el_max
        let mut t1 = t0 + step;
        let end = loop {
            if (t1 - start) > Duration::hours(1) {
                return Ok(None);
            }
            let observation = self.predictor.observe_at(t1, self.observer)?;
            if observation.elevation < self.min_elevation {
                // Transitioned out of a transit, refine transit end
                let end =
                    refine_crossing(time::datetime_to_f64(t0), time::datetime_to_f64(t1), &mut f)?;
                break time::f64_to_datetime(end);
            };
            t0 = t1;
            t1 += step;
        };
        // Update the state and next time so the next iteration picks up from here
        (self.next_time, *new_state) = (t1, TransitState::Outside(t1));
        Ok(Some(Transit::new(start, end)))
    }

    fn calculate_elevation(&self, t: DateTime<Utc>) -> Result<(f64, f64)> {
        let (el, el_rate) = self
            .predictor
            .propagate(t)?
            .to_ecef(t)
            .to_enu(self.observer)
            .elevation_and_rate();
        Ok((el, el_rate))
    }

    fn step_size(&self, el: f64, el_rate: f64) -> Duration {
        if el_rate <= 0.0 {
            // Descending portion of orbit, use max step
            MAX_STEP
        } else {
            Duration::seconds(((self.min_elevation - el) / el_rate) as i64)
                .clamp(MIN_STEP, MAX_STEP)
        }
    }
}

impl<'a, O: Observer> Iterator for TransitIter<'a, O> {
    type Item = Result<Transit>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.interval.contains(&self.next_time) {
            // Calculate observation at current time
            let t = self.next_time;
            let (el, el_rate) = match self.calculate_elevation(t) {
                Ok((el, el_rate)) => (el, el_rate),
                Err(e) => return Some(Err(e)),
            };
            let mut new_state = if el >= self.min_elevation {
                TransitState::Inside(t, el)
            } else {
                TransitState::Outside(t)
            };

            // Detect transit, if any. Calculate step size based on result.
            let result = match self.detect_transit(&mut new_state) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            // Calculate next step size
            self.next_time += self.step_size(el, el_rate);
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
    Inside(DateTime<Utc>, f64),
    Outside(DateTime<Utc>),
}

fn refine_crossing<F, E>(t0: f64, t1: f64, mut f: F) -> Result<f64>
where
    F: FnMut(f64) -> std::result::Result<(f64, f64), E>,
    E: std::error::Error,
{
    let t = (t0 + t1) / 2.0;

    // Try Newton-Raphson first; on cost-function error propagate immediately rather than
    // falling through to Brent (the same evaluation point would fail there too).
    match roots::newton_raphson(t, &mut f, 1e-3, 20) {
        Ok(root) => return Ok(root),
        Err(e @ roots::Error::CostFn(_)) => return Err(Error::Roots(e)),
        Err(_) => {} // convergence failure, fall through to Brent
    }

    // Fall back to Brent
    roots::brent(t0, t1, |x| f(x).map(|(el, _)| el), 1e-3, 50).map_err(Error::Roots)
}
