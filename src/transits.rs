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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HasId, HasTle, Predictor, roots::{Brent, NewtonRaphson, Refinement}};
    use chrono::{TimeZone, Utc};
    use std::convert::Infallible;

    // --- refine_crossing ---
    // These tests use synthetic elevation functions and need no Predictor.

    #[test]
    fn test_refine_crossing_newton_raphson_converges() {
        // Linear f(x) = x − 0.5: Newton-Raphson should converge in one step
        // from the midpoint of [0, 1].
        let result = refine_crossing(
            0.0,
            1.0,
            |x| Ok::<_, Infallible>((x - 0.5, 1.0)),
            &Refinement::default(),
        );
        assert!(result.is_ok());
        assert!((result.unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_refine_crossing_falls_back_to_brent_on_unstable() {
        // Derivative is always zero → Newton-Raphson returns Unstable.
        // Brent must find the root of f(x) = x − 0.5 in [0, 2].
        // (Midpoint is 1.0; f(1.0) = 0.5 ≠ 0, so NR won't converge first.)
        let result = refine_crossing(
            0.0,
            2.0,
            |x| Ok::<_, Infallible>((x - 0.5, 0.0)),
            &Refinement::default(),
        );
        assert!(result.is_ok(), "Brent fallback should succeed: {result:?}");
        assert!((result.unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_refine_crossing_falls_back_to_brent_on_max_iter() {
        // Newton-Raphson limited to 1 iteration won't converge on a cubic;
        // Brent must pick up and find the root of x³ − 0.5 in [0, 1].
        let refinement = Refinement {
            newton_raphson: NewtonRaphson { tolerance: 1e-6, max_iter: 1 },
            brent: Brent::default(),
        };
        let result = refine_crossing(
            0.0,
            1.0,
            |x| Ok::<_, Infallible>((x.powi(3) - 0.5, 3.0 * x.powi(2))),
            &refinement,
        );
        assert!(result.is_ok(), "Brent fallback should succeed: {result:?}");
        // root is 0.5^(1/3) ≈ 0.7937
        assert!((result.unwrap() - 0.5_f64.cbrt()).abs() < 1e-6);
    }

    #[test]
    fn test_refine_crossing_cost_fn_error_propagates() {
        // A cost-function error on the first NR evaluation must surface
        // immediately rather than falling through to Brent.
        #[derive(Debug)]
        struct CostErr;
        impl std::fmt::Display for CostErr {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "cost error")
            }
        }
        impl std::error::Error for CostErr {}

        let result = refine_crossing(
            0.0,
            1.0,
            |_| Err::<(f64, f64), CostErr>(CostErr),
            &Refinement::default(),
        );
        assert!(result.is_err());
    }

    // --- step_size ---

    struct TestSat;
    impl HasId for TestSat {
        fn id(&self) -> &str { "SENTINEL-2C" }
    }
    impl HasTle for TestSat {
        fn line_1(&self) -> &str {
            "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990"
        }
        fn line_2(&self) -> &str {
            "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
        }
    }

    struct TestObs;
    impl Observer for TestObs {
        fn latitude(&self) -> f64 { 0.0 }
        fn longitude(&self) -> f64 { 0.0 }
        fn altitude(&self) -> f64 { 0.0 }
    }

    fn make_iter(min_elevation_deg: f64) -> TransitIter<'static, TestObs> {
        static OBS: TestObs = TestObs;
        let predictor = Predictor::new(&TestSat).unwrap();
        let t = Utc.with_ymd_and_hms(2025, 12, 22, 0, 0, 0).unwrap();
        TransitIter::new(predictor, &OBS, t..(t + chrono::Duration::hours(1)), min_elevation_deg.to_radians())
    }

    #[test]
    fn test_step_size_descending_uses_max_step() {
        // el_rate ≤ 0 → always use MAX_STEP regardless of current elevation.
        let iter = make_iter(5.0);
        assert_eq!(iter.step_size(0.0, -0.01), MAX_STEP);
        assert_eq!(iter.step_size(0.0, 0.0), MAX_STEP);
    }

    #[test]
    fn test_step_size_large_gap_clamps_to_max() {
        // Satellite far below horizon rising slowly → formula produces > MAX_STEP → clamped.
        let iter = make_iter(5.0);
        let el = (-60_f64).to_radians();
        let el_rate = 0.0001; // rad/s — very slow rise
        assert_eq!(iter.step_size(el, el_rate), MAX_STEP);
    }

    #[test]
    fn test_step_size_near_horizon_clamps_to_min() {
        // Satellite just below min-elevation rising quickly → formula < MIN_STEP → clamped.
        let min_el = 5_f64.to_radians();
        let iter = make_iter(5.0);
        let el = min_el - 0.0001; // 0.1 mrad below threshold
        let el_rate = 1.0;        // 1 rad/s — very fast rise
        assert_eq!(iter.step_size(el, el_rate), MIN_STEP);
    }

    #[test]
    fn test_step_size_mid_range() {
        // Satellite 3° below min-elevation rising at 0.001 rad/s:
        //   (3° in rad) / 0.001 ≈ 52 s — well within (MIN_STEP=10s, MAX_STEP=600s).
        let min_el = 5_f64.to_radians();
        let iter = make_iter(5.0);
        let el = min_el - 3_f64.to_radians(); // 3° below threshold
        let el_rate = 0.001;                   // rad/s
        let step = iter.step_size(el, el_rate);
        assert!(step > MIN_STEP && step < MAX_STEP, "expected mid-range step, got {step:?}");
    }
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
