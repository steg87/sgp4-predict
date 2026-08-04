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
    angle::Radians,
    detect::{
        self, Direction, EventFunction, EventIter, FixedStep, MIN_POSITIVE_STEP, Sample,
        ThresholdStep, WindowIter,
    },
    observe::{Observation, Observer},
    roots::Refinement,
    time,
    time::IntervalRange,
};

/// A satellite pass — the window during which the satellite is above
/// `min_elevation` as seen from the observer.
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators to cover a specific pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Tuning knobs for [`TransitIter`]'s coarse scan and window walk.
///
/// Pass a customised value to
/// [`Predictor::transits_iter_with_opts`](crate::Predictor::transits_iter_with_opts).
#[derive(Debug, Clone, Copy)]
pub struct TransitIterOpts {
    /// Lower bound of the adaptive coarse-scan step (`ThresholdStep::min`).
    pub min_step: Duration,
    /// Upper bound of the adaptive coarse-scan step (`ThresholdStep::max`).
    pub max_step: Duration,
    /// Fixed step used to walk from a transit's start to its end; an
    /// adaptive step could jump clear over the peak and out the far side.
    pub walk_step: Duration,
    /// A transit longer than this is reported as
    /// [`DetectError::WindowTooLong`](crate::DetectError::WindowTooLong).
    pub max_transit_duration: Duration,
    /// A transit already in progress at the interval start is discarded by
    /// default (only transits whose AOS falls within the interval are
    /// returned); set to `false` to instead walk backward past the interval
    /// start and find its true AOS.
    pub skip_leading_partial: bool,
    /// A transit still in progress at the interval end is walked forward
    /// past the interval to find its true LOS by default; set to `true` to
    /// instead clamp its end to the interval bounds.
    pub clamp_to_interval: bool,
}

impl Default for TransitIterOpts {
    fn default() -> Self {
        Self {
            min_step: Duration::seconds(10),
            max_step: Duration::minutes(10),
            walk_step: Duration::seconds(30),
            max_transit_duration: Duration::hours(1),
            skip_leading_partial: true,
            clamp_to_interval: false,
        }
    }
}

/// Tuning knobs for [`Predictor::max_elevation`]'s scan.
///
/// Pass a customised value to
/// [`Predictor::max_elevation_with_opts`](crate::Predictor::max_elevation_with_opts).
#[derive(Debug, Clone, Copy)]
pub struct MaxElevationOpts {
    /// Fixed step used to scan for elevation-rate zero crossings.
    pub scan_step: Duration,
}

impl Default for MaxElevationOpts {
    fn default() -> Self {
        Self {
            scan_step: Duration::seconds(10),
        }
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
        opts: TransitIterOpts,
        refinement: Refinement,
    ) -> Self {
        let mut builder = WindowIter::builder()
            .interval(interval)
            .event_function(ElevationAboveMin {
                predictor,
                observer,
                min_elevation,
            })
            .step(ThresholdStep {
                min: opts.min_step.max(MIN_POSITIVE_STEP),
                max: opts.max_step.max(MIN_POSITIVE_STEP),
            })
            .walk_step(opts.walk_step)
            .max_window_duration(opts.max_transit_duration)
            .refinement(refinement);
        if !opts.skip_leading_partial {
            builder = builder.include_leading_partial();
        }
        if opts.clamp_to_interval {
            builder = builder.clamp_to_interval();
        }
        let inner = builder.build().expect("interval is always supplied");
        Self { inner }
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
    /// `min_elevation` is the minimum elevation above the horizon — pass a
    /// [`Degrees`](crate::Degrees) or [`Radians`] value directly.
    ///
    /// Returns an iterator over transits.
    pub fn transits_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: impl Into<Radians>,
    ) -> TransitIter<'a, O> {
        self.transits_iter_with_opts(
            observer,
            interval,
            min_elevation,
            TransitIterOpts::default(),
            self.refinement,
        )
    }

    /// Like [`Predictor::transits_iter`], but with a customized root-finder
    /// configuration and coarse-scan/window-walk tuning. See [`Refinement`]
    /// and [`TransitIterOpts`].
    pub fn transits_iter_with_opts<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: impl Into<Radians>,
        opts: TransitIterOpts,
        refinement: Refinement,
    ) -> TransitIter<'a, O> {
        TransitIter::new(
            self.clone(),
            observer,
            interval,
            min_elevation.into().to_f64(),
            opts,
            refinement,
        )
    }

    /// Detect whether a transit is in progress at time `t`.
    ///
    /// `min_elevation` is the minimum elevation above the horizon — pass a
    /// [`Degrees`](crate::Degrees) or [`Radians`] value directly.
    ///
    /// If the satellite is below `min_elevation` at `t`, returns
    /// `Ok(None)`. Otherwise, walks backward and forward from `t` using
    /// [`TransitIterOpts::default`]'s `walk_step` to find the AoS and LoS
    /// crossings, refining each with the bracketed hybrid solver
    /// ([`Refinement`]) to millisecond accuracy — see `detect_window`, the
    /// primitive this is a thin wrapper over.
    ///
    /// Returns [`Error::Detect`](crate::Error::Detect) if the transit is
    /// longer than [`TransitIterOpts::default`]'s `max_transit_duration`.
    pub fn detect_transit<O: Observer>(
        &self,
        t: DateTime<Utc>,
        observer: &O,
        min_elevation: impl Into<Radians>,
    ) -> Result<Option<Transit>> {
        self.detect_transit_with_opts(t, observer, min_elevation, TransitIterOpts::default())
    }

    /// Like [`Predictor::detect_transit`], but with a customized walk step
    /// and max transit duration. Only [`TransitIterOpts::walk_step`] and
    /// [`TransitIterOpts::max_transit_duration`] are used — the other fields
    /// don't apply to this single-point detection.
    pub fn detect_transit_with_opts<O: Observer>(
        &self,
        t: DateTime<Utc>,
        observer: &O,
        min_elevation: impl Into<Radians>,
        opts: TransitIterOpts,
    ) -> Result<Option<Transit>> {
        let mut f = ElevationAboveMin {
            predictor: self.clone(),
            observer,
            min_elevation: min_elevation.into().to_f64(),
        };
        let window = detect::detect_window(
            &mut f,
            t,
            opts.walk_step,
            opts.max_transit_duration,
            &self.refinement,
        )?;
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
        self.max_elevation_with_opts(interval, observer, MaxElevationOpts::default())
    }

    /// Like [`Predictor::max_elevation`], but with a customized scan step.
    /// See [`MaxElevationOpts`].
    pub fn max_elevation_with_opts<O: Observer>(
        &self,
        interval: impl IntervalRange,
        observer: &O,
        opts: MaxElevationOpts,
    ) -> Result<(DateTime<Utc>, Observation)> {
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
            .step(FixedStep(opts.scan_step.max(MIN_POSITIVE_STEP)))
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
            elevation_deg = obs.elevation.degrees(),
            "peak elevation found"
        );
        Ok((peak_t, obs))
    }
}
