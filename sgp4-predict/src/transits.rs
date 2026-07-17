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
    observe::{Observation, Observer},
    roots,
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
    /// If the satellite is below `min_elevation_deg` at `t`, returns `Ok(None)`.
    /// Otherwise, searches backward and forward in 30-second steps to bracket the
    /// AoS and LoS crossings, then refines each boundary with the bracketed
    /// hybrid solver ([`Refinement`](crate::Refinement)) to millisecond accuracy.
    ///
    /// Returns an error if either boundary is not found within 1 hour.
    pub fn detect_transit<O: Observer>(
        &self,
        t: DateTime<Utc>,
        observer: &O,
        min_elevation_deg: f64,
    ) -> Result<Option<Transit>> {
        let min_elevation = min_elevation_deg.to_radians();
        let calculate = |t: DateTime<Utc>| -> Result<(f64, f64)> {
            let (el, el_rate) = self
                .propagate(t)?
                .to_ecef(t)
                .to_enu(observer)
                .elevation_and_rate();
            Ok((el, el_rate))
        };

        let mut f = |t: f64| {
            calculate(time::f64_to_datetime(t))
                .map(|(el, el_rate)| (el - min_elevation, Some(el_rate)))
        };

        let (el, _) = calculate(t)?;
        if el < min_elevation {
            return Ok(None);
        }

        const STEP: Duration = Duration::seconds(30);

        // --- Find start (search backward) ---
        let mut t_inner = t;
        let mut t_outer = t - STEP;
        let start = loop {
            if t - t_outer > Duration::hours(1) {
                tracing::warn!(at = %t, "transit start not found within 1 hour");
                return Err(Error::TransitStartNotFound { at: t }.into());
            }
            let (el, _) = calculate(t_outer)?;
            if el < min_elevation {
                let s = self.refinement.solve(
                    time::datetime_to_f64(t_outer),
                    time::datetime_to_f64(t_inner),
                    &mut f,
                )?;
                break time::f64_to_datetime(s);
            }
            t_inner = t_outer;
            t_outer -= STEP;
        };

        // --- Find end (search forward) ---
        let mut t_inner = t;
        let mut t_outer = t + STEP;
        let end = loop {
            if t_outer - t > Duration::hours(1) {
                tracing::warn!(%start, "transit end not found within 1 hour");
                return Err(Error::TransitEndNotFound { start }.into());
            }
            let (el, _) = calculate(t_outer)?;
            if el < min_elevation {
                let e = self.refinement.solve(
                    time::datetime_to_f64(t_inner),
                    time::datetime_to_f64(t_outer),
                    &mut f,
                )?;
                break time::f64_to_datetime(e);
            }
            t_inner = t_outer;
            t_outer += STEP;
        };

        let transit = Transit::new(start, end);
        tracing::debug!(aos = %transit.start, los = %transit.end, "transit detected");
        Ok(Some(transit))
    }

    /// Find the peak elevation of the satellite over an observer within a time interval.
    ///
    /// Scans in 10-second steps to bracket the point where the elevation rate crosses
    /// zero (ascending → descending), then refines the crossing with the bracketed
    /// hybrid solver ([`Refinement`](crate::Refinement)).
    /// If no sign change is found (satellite never peaks within the interval), a
    /// roots::Error::Unbracketed is returned.
    pub fn max_elevation<O: Observer>(
        &self,
        interval: impl IntervalRange,
        observer: &O,
    ) -> Result<(DateTime<Utc>, Observation)> {
        const SCAN_STEP: Duration = Duration::seconds(10);
        let start_t = interval.start();
        let end_t = interval.end();

        let mut prev: Option<(f64, f64)> = None; // (t_f64, el_rate)
        let mut t = start_t;

        while t <= end_t {
            let t_f64 = time::datetime_to_f64(t);
            let (_, el_rate) = self
                .propagate(t)?
                .to_ecef(t)
                .to_enu(observer)
                .elevation_and_rate();

            if let Some((prev_t, prev_er)) = prev
                && prev_er > 0.0
                && el_rate < 0.0
            {
                // el_rate crossed zero: peak is bracketed in [prev_t, t_f64].
                // The event function here is the elevation *rate*, whose own
                // derivative is not available — samples carry no rate.
                let peak_t_f64 = self.refinement.solve(prev_t, t_f64, |x| {
                    let tx = time::f64_to_datetime(x);
                    self.propagate(tx)
                        .map(|s| (s.to_ecef(tx).to_enu(observer).elevation_and_rate().1, None))
                })?;

                let peak_t = time::f64_to_datetime(peak_t_f64);
                let obs = self.observe_at(peak_t, observer)?;
                tracing::debug!(time = %peak_t, elevation_deg = obs.elevation.to_degrees(), "peak elevation found");
                return Ok((peak_t, obs));
            }

            prev = Some((t_f64, el_rate));
            t += SCAN_STEP;
        }

        // No sign change found — no peak within the interval
        Err(roots::Error::Unbracketed.into())
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
