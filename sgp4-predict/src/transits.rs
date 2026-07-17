//! Transit (satellite pass) detection and iteration.
//!
//! [`TransitIter`] uses an adaptive step-size strategy to scan efficiently:
//! large steps when the satellite is descending or far below `min_elevation`,
//! smaller steps as it approaches. Each Outside→Inside transition is refined
//! to millisecond accuracy with the bracketed hybrid solver.
//!
//! It is a thin wrapper over the generic [`WindowIter`](crate::WindowIter):
//! the event function is `elevation − min_elevation` and transits are the
//! windows where it is positive.
//!
//! A [`Transit`] also implements [`IntervalRange`], so it can be passed
//! directly to [`Predictor::prediction_iter`] or [`Predictor::observation_iter`]
//! to iterate over a specific pass.
//!
//! [`IntervalRange`]: crate::IntervalRange
//! [`Predictor::prediction_iter`]: crate::Predictor::prediction_iter
//! [`Predictor::observation_iter`]: crate::Predictor::observation_iter

use chrono::{DateTime, Duration, Utc};
use thiserror::Error as ThisError;

use crate::{
    Predictor, Result,
    detect::{EventFunction, Sample, ThresholdStep, WindowIter},
    observe::Observer,
    roots::Refinement,
    time,
    time::IntervalRange,
};

const MAX_STEP: Duration = Duration::minutes(10);
const MIN_STEP: Duration = Duration::seconds(10);
/// Fixed step used to walk from a transit's start to its end; an adaptive
/// step could jump clear over the peak and out the far side.
const WALK_STEP: Duration = Duration::seconds(30);
/// A transit longer than this is reported as
/// [`DetectError::WindowEndNotFound`](crate::DetectError::WindowEndNotFound).
const WALK_TIMEOUT: Duration = Duration::hours(1);

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

    /// Returns a copy of this transit clamped to `interval`, or `None` if the
    /// transit lies entirely outside the interval.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use sgp4_predict::Transit;
    ///
    /// let transit = Transit::new(
    ///     Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
    ///     Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap(),
    /// );
    /// let window = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 30, 0).unwrap();
    ///
    /// let clamped = transit.clamp(&window).unwrap();
    /// assert_eq!(clamped.start, Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap());
    /// assert_eq!(clamped.end,   Utc.with_ymd_and_hms(2024, 1, 1, 1,  0, 0).unwrap());
    ///
    /// // Fully outside returns None.
    /// let disjoint = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 3, 0, 0).unwrap();
    /// assert!(transit.clamp(&disjoint).is_none());
    /// ```
    pub fn clamp(&self, interval: &impl time::IntervalRange) -> Option<Transit> {
        self.intersection(interval)
            .map(|r| Transit::new(r.start, r.end))
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

/// Event function: the satellite's elevation above `min_elevation`, with its
/// rate of change as the derivative (enabling Newton-Raphson refinement and
/// adaptive stepping).
pub(crate) struct ElevationAboveMin<'a, O: Observer> {
    predictor: Predictor,
    observer: &'a O,
    min_elevation: f64,
}

impl<'a, O: Observer> EventFunction for ElevationAboveMin<'a, O> {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        let (el, el_rate) = self
            .predictor
            .propagate(t)?
            .to_ecef(t)
            .to_enu(self.observer)
            .elevation_and_rate();
        Ok(Sample {
            time: t,
            value: el - self.min_elevation,
            rate: Some(el_rate),
        })
    }
}

/// Iterator over satellite passes visible to an observer within a time interval.
///
/// Created by [`Predictor::transits_iter`](crate::Predictor::transits_iter).
pub struct TransitIter<'a, O: Observer> {
    inner: WindowIter<ElevationAboveMin<'a, O>, ThresholdStep>,
}

impl<'a, O: Observer> TransitIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl time::IntervalRange,
        min_elevation: f64,
    ) -> Self {
        let inner = WindowIter::builder()
            .interval(interval)
            .event_function(ElevationAboveMin {
                predictor,
                observer,
                min_elevation,
            })
            .step(ThresholdStep {
                min: MIN_STEP,
                max: MAX_STEP,
            })
            .emit_positive_only()
            .skip_leading_partial()
            .scan_past_end(WALK_STEP, WALK_TIMEOUT)
            .build()
            .expect("interval is always supplied");
        Self { inner }
    }

    pub fn with_refinement(mut self, r: Refinement) -> Self {
        *self.inner.detector_mut().refinement_mut() = r;
        self
    }
}

impl<'a, O: Observer> Iterator for TransitIter<'a, O> {
    type Item = Result<Transit>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.inner.next()?.map(|window| {
            let transit = Transit::new(window.start, window.end);
            tracing::debug!(aos = %transit.start, los = %transit.end, "transit detected");
            transit
        }))
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
