//! Transit (satellite pass) detection and iteration.
//!
//! [`TransitIter`] uses an adaptive step-size strategy to scan efficiently:
//! large steps when the satellite is descending or far below `min_elevation`,
//! smaller steps as it approaches. Each Outside→Inside transition is refined
//! to millisecond accuracy using Newton-Raphson with a Brent fallback.
//!
//! A [`Transit`] also implements [`IntervalRange`], so it can be passed
//! directly to [`Predictor::prediction_iter`] or [`Predictor::observation_iter`]
//! to iterate over a specific pass.
//!
//! [`IntervalRange`]: crate::IntervalRange
//! [`Predictor::prediction_iter`]: crate::Predictor::prediction_iter
//! [`Predictor::observation_iter`]: crate::Predictor::observation_iter

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;
use thiserror::Error as ThisError;

use crate::{Error as LibError, Predictor, Result, observe::Observer, roots, time};
use roots::Refinement;

const MAX_STEP: Duration = Duration::minutes(10);
const MIN_STEP: Duration = Duration::seconds(10);

/// A satellite pass — the window during which the satellite is above
/// `min_elevation` as seen from the observer.
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators to cover a specific pass.
#[derive(Debug, Clone, Copy)]
pub struct Transit {
    /// Acquisition of Signal: when the satellite rises above `min_elevation`.
    pub start: DateTime<Utc>,
    /// Loss of Signal: when the satellite drops below `min_elevation`.
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

/// Iterator over satellite passes visible to an observer within a time interval.
///
/// Created by [`Predictor::transits_iter`](crate::Predictor::transits_iter).
pub struct TransitIter<'a, O: Observer> {
    predictor: Predictor,
    observer: &'a O,
    interval: Range<DateTime<Utc>>,
    min_elevation: f64,
    next_time: DateTime<Utc>,
    state: Option<TransitState>,
    refinement: Refinement,
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
            refinement: Refinement::default(),
        }
    }

    pub fn with_refinement(mut self, r: Refinement) -> Self {
        self.refinement = r;
        self
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
                    (TransitState::Outside(t0), TransitState::Inside(t1)) => {
                        // Transitioned into a transit, refine transit start
                        let start = refine_crossing(
                            time::datetime_to_f64(*t0),
                            time::datetime_to_f64(*t1),
                            &mut f,
                            &self.refinement,
                        )?;
                        time::f64_to_datetime(start)
                    }
                    _ => return Ok(None), // No other state transitions of interest
                }
            }
            None => {
                // If the satellite is already inside a transit at the start of the window,
                // that transit began before the window and is not returned. Subsequent
                // Outside→Inside transitions will be detected normally.
                return Ok(None);
            }
        };

        // new_state must be Inside at this point, advance time until the state is Outside
        let mut t0 = start;
        let step = Duration::seconds(30); // Fixed step, el_rate won't help cross el_max
        let mut t1 = t0 + step;
        let end = loop {
            if (t1 - start) > Duration::hours(1) {
                return Err(Error::TransitEndNotFound { start }.into());
            }
            let observation = self.predictor.observe_at(t1, self.observer)?;
            if observation.elevation < self.min_elevation {
                // Transitioned out of a transit, refine transit end
                let end = refine_crossing(
                    time::datetime_to_f64(t0),
                    time::datetime_to_f64(t1),
                    &mut f,
                    &self.refinement,
                )?;
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
                TransitState::Inside(t)
            } else {
                TransitState::Outside(t)
            };

            // Detect transit, if any. Calculate step size based on result.
            let result = match self.detect_transit(&mut new_state) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            // Update current state
            self.state.replace(new_state);
            // If a transit was found, detect_transit already advanced next_time to the
            // first confirmed outside sample; no further step needed.
            // Otherwise, advance by an adaptive step based on current elevation and rate.
            if let Some(transit) = result {
                return Some(Ok(transit));
            }
            self.next_time += self.step_size(el, el_rate);
        }
        None
    }
}

#[derive(Debug, Clone)]
enum TransitState {
    Inside(DateTime<Utc>),
    Outside(DateTime<Utc>),
}

pub(crate) fn refine_crossing<F, E>(
    t0: f64,
    t1: f64,
    mut f: F,
    refinement: &Refinement,
) -> Result<f64>
where
    F: FnMut(f64) -> std::result::Result<(f64, f64), E>,
    E: std::error::Error,
{
    let t = (t0 + t1) / 2.0;

    // Try Newton-Raphson first; on cost-function error propagate immediately rather than
    // falling through to Brent (the same evaluation point would fail there too).
    // Tolerance is on the elevation function value (radians). At a typical AoS/LoS
    // elevation rate of ~2 mrad/s, 1e-6 rad gives < 1 ms time precision.
    match refinement.newton_raphson.solve(t, &mut f) {
        Ok(root) => return Ok(root),
        Err(e @ roots::Error::CostFn(_)) => return Err(LibError::Roots(e)),
        Err(_) => {} // convergence failure, fall through to Brent
    }

    // Fall back to Brent
    refinement
        .brent
        .solve(t0, t1, |x| f(x).map(|(el, _)| el))
        .map_err(LibError::Roots)
}

/// Errors that can occur during transit detection.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error(
        "transit end not found: satellite remained above minimum elevation \
        for more than 1 hour from {start}"
    )]
    TransitEndNotFound { start: DateTime<Utc> },
    #[error(
        "transit start not found: satellite remained above minimum elevation \
         for more than 1 hour before {at}"
    )]
    TransitStartNotFound { at: DateTime<Utc> },
}
