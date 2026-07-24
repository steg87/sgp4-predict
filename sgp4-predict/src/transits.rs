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

use crate::{
    Predictor, Result,
    detect::{
        self, Direction, EventFunction, EventIter, FixedStep, Sample, ThresholdStep, WindowIter,
    },
    observe::{Observation, Observer},
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
/// [`DetectError::WindowTooLong`](crate::DetectError::WindowTooLong).
const MAX_TRANSIT_DURATION: Duration = Duration::hours(1);

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
            .walk_step(WALK_STEP)
            .max_window_duration(MAX_TRANSIT_DURATION)
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

impl Predictor {
    /// Calculate all of the transits visible to the observer.
    ///
    /// `min_elevation_deg` is the minimum elevation above the horizon in **degrees**.
    ///
    /// Returns an iterator over transits.
    pub fn transits_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation_deg: f64,
    ) -> TransitIter<'a, O> {
        TransitIter::new(
            self.clone(),
            observer,
            interval,
            min_elevation_deg.to_radians(),
        )
        .with_refinement(self.refinement)
    }

    /// Detect whether a transit is in progress at time `t`.
    ///
    /// `min_elevation_deg` is the minimum elevation above the horizon in **degrees**.
    ///
    /// If the satellite is below `min_elevation_deg` at `t`, returns
    /// `Ok(None)`. Otherwise, walks backward and forward from `t` in
    /// 30-second steps to find the AoS and LoS crossings, refining each with
    /// the bracketed hybrid solver ([`Refinement`]) to millisecond accuracy —
    /// see `detect_window`, the primitive this is a thin wrapper over.
    ///
    /// Returns [`Error::Detect`](crate::Error::Detect) if the transit is
    /// longer than 1 hour.
    pub fn detect_transit<O: Observer>(
        &self,
        t: DateTime<Utc>,
        observer: &O,
        min_elevation_deg: f64,
    ) -> Result<Option<Transit>> {
        let mut f = ElevationAboveMin {
            predictor: self.clone(),
            observer,
            min_elevation: min_elevation_deg.to_radians(),
        };
        let window =
            detect::detect_window(&mut f, t, WALK_STEP, MAX_TRANSIT_DURATION, &self.refinement)?;
        Ok(window.map(|w| {
            let transit = Transit::new(w.start, w.end);
            tracing::debug!(aos = %transit.start, los = %transit.end, "transit detected");
            transit
        }))
    }

    /// Find the peak elevation of the satellite over an observer within a time interval.
    ///
    /// Built on [`EventIter`]: the event function is the elevation rate, and
    /// interior peaks are its falling zero crossings (ascending →
    /// descending), refined with the bracketed hybrid solver
    /// ([`Refinement`]). The global maximum over the interval is attained
    /// either at one of these interior peaks or at an interval boundary, so
    /// every candidate — each falling crossing plus both endpoints — is
    /// compared and the highest returned.
    pub fn max_elevation<O: Observer>(
        &self,
        interval: impl IntervalRange,
        observer: &O,
    ) -> Result<(DateTime<Utc>, Observation)> {
        const SCAN_STEP: Duration = Duration::seconds(10);
        let start_t = interval.start();
        let end_t = interval.end();

        let crossings = EventIter::builder()
            .interval(interval)
            // Check for crossings when elevation rate is zero
            .function(|t| {
                Ok(self
                    .propagate(t)?
                    .to_ecef(t)
                    .to_enu(observer)
                    .elevation_and_rate()
                    .1)
            })
            .step(FixedStep(SCAN_STEP))
            .refinement(self.refinement)
            .build()
            .expect("interval is always supplied");

        let (peak_t, obs) = crossings
            // Only consider ascending -> descending crossings
            .filter_map(|c| match c {
                Ok(c) if c.direction == Direction::Falling => Some(Ok(c.time)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            })
            // Add start and end times, in case there are no crossings within the interval
            .chain([Ok(start_t), Ok(end_t)])
            // Calculate elevation at each time
            .map(|t| -> Result<_> {
                let t = t?;
                Ok((t, self.observe_at(t, observer)?))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            // Find max elevation
            .max_by(|a, b| a.1.elevation.total_cmp(&b.1.elevation))
            .expect("candidates always include the interval endpoints");

        tracing::debug!(
            time = %peak_t,
            elevation_deg = obs.elevation.to_degrees(),
            "peak elevation found"
        );
        Ok((peak_t, obs))
    }
}
