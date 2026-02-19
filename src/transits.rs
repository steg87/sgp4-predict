use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, observe::Observer, roots, time};

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
    fn detect_transit(&mut self, new_state: &mut TransitState) -> Option<Transit> {
        // Define refinement cost function closure
        let mut f = |t| {
            let t = DateTime::from_timestamp(t as i64, 0).unwrap();
            let (el, el_rate) = self.calculate_elevation(t).unwrap(); // TODO
            (el - self.min_elevation, el_rate)
        };

        // Determine if state transition indicates that a new transit has been found
        let start = match &self.state {
            // Previous example exists, check if we have transitioned into a transit
            Some(prev_state) => {
                match (prev_state, &*new_state) {
                    (TransitState::Outside(t0), TransitState::Inside(t1, _)) => {
                        // Transitioned into a transit, refine transit start and return
                        let start = refine_crossing(
                            time::datetime_to_f64(*t0),
                            time::datetime_to_f64(*t1),
                            &mut f,
                        )
                        .ok()?;
                        time::f64_to_datetime(start)
                    }
                    _ => return None, // No other state transitions of interest
                }
            }
            None => {
                match &*new_state {
                    TransitState::Inside(t1, el) if *el == self.min_elevation => {
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
        let step = Duration::seconds(30); // Fixed step, el_rate won't help cross el_max
        let mut t1 = t0 + step;
        let end = loop {
            if (t1 - start) > Duration::hours(1) {
                // TODO: log warning transit was longer than an hour and was ignored
                return None;
            }
            let observation = self.predictor.observe_at(t1, self.observer).unwrap(); // TODO
            if observation.elevation < self.min_elevation {
                // Transitioned out of a transit, refine transit end and return
                let end =
                    refine_crossing(time::datetime_to_f64(t0), time::datetime_to_f64(t1), &mut f)
                        .ok()?;
                break time::f64_to_datetime(end);
            };
            t0 = t1;
            t1 += step;
        };
        // Update the state and next time so the next iteration picks up from here
        (self.next_time, *new_state) = (t1, TransitState::Outside(t1));
        Some(Transit::new(start, end))
    }

    fn calculate_elevation(&self, t: DateTime<Utc>) -> Result<(f64, f64), Error> {
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
    type Item = Result<Transit, Error>;

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
            let result = self.detect_transit(&mut new_state);
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

fn refine_crossing<F>(t0: f64, t1: f64, mut f: F) -> Result<f64, Error>
where
    F: FnMut(f64) -> (f64, f64),
{
    let t = (t0 + t1) / 2.0;

    // Try Newton-Raphson first
    let result = roots::newton_raphson(t, &mut f, 1e-3, 50)
        // TODO: log Newton-Raphson failure
        // Fall back to Brent if Newton-Raphson fails
        .or_else(|_| roots::brent(t0, t1, |x| f(x).0, 1e-3, 100))
        // TODO: log Brent failure
        .map_err(Error::Roots)?;
    Ok(result)
}
