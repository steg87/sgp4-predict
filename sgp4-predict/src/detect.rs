//! Generic event and window detection over scalar functions of time.
//!
//! This module contains the building blocks that power [`ApsisIter`],
//! [`TransitIter`] and [`IlluminationIter`], exposed so that new kinds of
//! satellite events can be detected without writing a bespoke iterator.
//!
//! # Layers
//!
//! - [`DetectIter`] is the generic driving loop: it repeatedly asks a
//!   [`Detector`] for the next sample time and whether an event completed
//!   there. Implement [`Detector`] directly for fully custom detection.
//! - [`CrossingDetector`] (built via [`EventIter::builder`]) is a provided
//!   detector that finds the zero crossings of a user-supplied scalar
//!   [`EventFunction`] `f(t)` — point-in-time **events** such as apsides.
//! - [`WindowDetector`] (built via [`WindowIter::builder`]) pairs crossings
//!   into **windows** — intervals over which `f(t)` keeps one sign, such as
//!   transits (elevation above a threshold) or illumination (shadow function).
//! - [`StepStrategy`] decides how far to advance between samples:
//!   [`FixedStep`] scans uniformly, [`ThresholdStep`] takes large steps far
//!   from a threshold crossing and small steps near it.
//!
//! Crossings bracketed by two samples are refined with the crate's root
//! finders: Newton-Raphson with a Brent fallback when the event function
//! supplies a derivative ([`Sample::rate`]), Brent's method alone otherwise.
//!
//! # Example: northward equator crossings
//!
//! In the TEME frame the equator is the plane `z = 0`, so ascending-node
//! crossings are the rising zero crossings of the satellite's z coordinate:
//!
//! ```no_run
//! use chrono::{Duration, Utc};
//! use sgp4_predict::{Direction, EventIter, FixedStep, Predictor, Tle};
//!
//! # let tle: Tle = "ISS (ZARYA)\n1 ...\n2 ...".parse().unwrap();
//! let predictor = Predictor::from_tle(&tle).unwrap();
//! let start = Utc::now();
//!
//! let crossings = EventIter::builder()
//!     .interval(start..start + Duration::days(1))
//!     .function(move |t| Ok(predictor.propagate(t)?.position.z))
//!     .step(FixedStep(Duration::seconds(60)))
//!     .build()
//!     .unwrap();
//!
//! for crossing in crossings {
//!     let crossing = crossing.unwrap();
//!     if crossing.direction == Direction::Rising {
//!         println!("northward equator crossing at {}", crossing.time);
//!     }
//! }
//! ```

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;
use thiserror::Error as ThisError;

use crate::{Result, roots::Refinement, time, time::IntervalRange};

/// One evaluation of an [`EventFunction`].
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Time at which the function was evaluated.
    pub time: DateTime<Utc>,
    /// Function value.
    pub value: f64,
    /// Time derivative of the value in 1/s, if cheaply available.
    ///
    /// When present, crossings are refined with Newton-Raphson (Brent
    /// fallback); when absent, with Brent's method alone.
    pub rate: Option<f64>,
}

/// A scalar function of time whose zero crossings define events.
///
/// Plain closures are adapted with [`ValueFn`] / [`RateFn`] (the
/// [`EventIterBuilder::function`] and
/// [`EventIterBuilder::function_with_rate`] builder methods do this for
/// you). Implement the trait directly to keep state or expose a nameable
/// type.
pub trait EventFunction {
    /// Evaluate the function at `t`.
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample>;
}

/// Adapts a `FnMut(DateTime<Utc>) -> Result<f64>` closure into an
/// [`EventFunction`] with no derivative.
pub struct ValueFn<F>(pub F);

impl<F: FnMut(DateTime<Utc>) -> Result<f64>> EventFunction for ValueFn<F> {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        Ok(Sample {
            time: t,
            value: (self.0)(t)?,
            rate: None,
        })
    }
}

/// Adapts a `FnMut(DateTime<Utc>) -> Result<(f64, f64)>` closure returning
/// `(value, rate)` into an [`EventFunction`] with a derivative.
pub struct RateFn<F>(pub F);

impl<F: FnMut(DateTime<Utc>) -> Result<(f64, f64)>> EventFunction for RateFn<F> {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        let (value, rate) = (self.0)(t)?;
        Ok(Sample {
            time: t,
            value,
            rate: Some(rate),
        })
    }
}

/// Strategy for choosing the next sample time.
pub trait StepStrategy {
    /// Return the next sample time after `current`.
    ///
    /// `sample` is the function evaluation taken at `current`, when that
    /// evaluation succeeded; strategies must still advance when it is `None`
    /// (e.g. after an evaluation error) so iteration cannot stall.
    fn next_time(&mut self, current: DateTime<Utc>, sample: Option<&Sample>) -> DateTime<Utc>;
}

/// Advance by a constant duration each step.
#[derive(Debug, Clone, Copy)]
pub struct FixedStep(pub Duration);

impl StepStrategy for FixedStep {
    fn next_time(&mut self, current: DateTime<Utc>, _sample: Option<&Sample>) -> DateTime<Utc> {
        current + self.0
    }
}

/// Adaptive stepping towards a rising zero crossing.
///
/// When the function is falling (or the rate is unavailable) the step is
/// `max`. When it is rising, the step is the estimated time to reach zero,
/// `-value / rate`, clamped to `[min, max]` — large steps far below the
/// threshold, small steps as the crossing approaches. This is the strategy
/// used by transit detection.
#[derive(Debug, Clone, Copy)]
pub struct ThresholdStep {
    /// Smallest step taken when the crossing is imminent.
    pub min: Duration,
    /// Largest step taken when far from the crossing or falling.
    pub max: Duration,
}

impl Default for ThresholdStep {
    fn default() -> Self {
        Self {
            min: Duration::seconds(10),
            max: Duration::minutes(10),
        }
    }
}

impl StepStrategy for ThresholdStep {
    fn next_time(&mut self, current: DateTime<Utc>, sample: Option<&Sample>) -> DateTime<Utc> {
        let step = match sample.and_then(|s| s.rate.map(|rate| (s.value, rate))) {
            Some((value, rate)) if rate > 0.0 => {
                Duration::seconds((-value / rate) as i64).clamp(self.min, self.max)
            }
            _ => self.max,
        };
        current + step
    }
}

/// A stateful detector driven by [`DetectIter`].
///
/// Implement this directly for detection logic that does not fit the
/// provided [`CrossingDetector`] / [`WindowDetector`].
pub trait Detector {
    /// The type of event this detector produces.
    type Event;

    /// Choose the next sample time given the current one. A stateful
    /// detector may use whatever it recorded during the last
    /// [`detect_event`](Detector::detect_event) call (e.g. a rate) to adapt
    /// the step.
    fn next_time(&mut self, current: DateTime<Utc>) -> DateTime<Utc>;

    /// Sample at `t`; report a completed event, if any.
    fn detect_event(&mut self, t: DateTime<Utc>) -> Result<Option<Self::Event>>;

    /// Called repeatedly once the interval is exhausted, until it returns
    /// `Ok(None)` — lets window detectors flush trailing partial windows.
    fn finish(&mut self, _end: DateTime<Utc>) -> Result<Option<Self::Event>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Scan,
    Finish,
    Done,
}

/// The generic detection loop: an iterator over the events found by a
/// [`Detector`] within a time interval.
///
/// Each iteration asks the detector for the next sample time (the first
/// sample lands on `interval.start()`), stops scanning once that time leaves
/// the interval, then drains [`Detector::finish`]. Errors are yielded as
/// items and iteration continues from the following sample.
pub struct DetectIter<D> {
    detector: D,
    interval: Range<DateTime<Utc>>,
    current: Option<DateTime<Utc>>,
    phase: Phase,
}

impl<D: Detector> DetectIter<D> {
    /// Create a detection loop over `interval` driven by `detector`.
    pub fn new(interval: impl IntervalRange, detector: D) -> Self {
        Self {
            detector,
            interval: interval.start()..interval.end(),
            current: None,
            phase: Phase::Scan,
        }
    }

    /// Access the underlying detector.
    pub fn detector(&self) -> &D {
        &self.detector
    }

    /// Mutably access the underlying detector (e.g. to adjust configuration
    /// before iterating).
    pub fn detector_mut(&mut self) -> &mut D {
        &mut self.detector
    }
}

impl<D: Detector> Iterator for DetectIter<D> {
    type Item = Result<D::Event>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.phase == Phase::Scan {
            let t = match self.current {
                None => self.interval.start,
                Some(current) => self.detector.next_time(current),
            };
            if !self.interval.contains(&t) {
                self.phase = Phase::Finish;
                break;
            }
            self.current = Some(t);
            match self.detector.detect_event(t) {
                Ok(Some(event)) => return Some(Ok(event)),
                Ok(None) => {}
                Err(e) => return Some(Err(e)),
            }
        }
        while self.phase == Phase::Finish {
            match self.detector.finish(self.interval.end) {
                Ok(Some(event)) => return Some(Ok(event)),
                Ok(None) => self.phase = Phase::Done,
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

/// The direction of a zero crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The function value went from negative to positive.
    Rising,
    /// The function value went from positive to negative.
    Falling,
}

/// A refined zero crossing of an [`EventFunction`].
#[derive(Debug, Clone, Copy)]
pub struct Crossing {
    /// Refined time of the crossing.
    pub time: DateTime<Utc>,
    /// Whether the function was rising or falling through zero.
    pub direction: Direction,
}

/// Refine the crossing bracketed by `[t0, t1]` with the configured root
/// finders: Newton-Raphson with Brent fallback when the function supplies a
/// rate, Brent's method alone otherwise.
fn refine<F: EventFunction>(
    f: &mut F,
    refinement: &Refinement,
    t0: DateTime<Utc>,
    t1: DateTime<Utc>,
    with_rate: bool,
) -> Result<DateTime<Utc>> {
    let a = time::datetime_to_f64(t0);
    let b = time::datetime_to_f64(t1);
    let root = if with_rate {
        refinement.hybrid_solve(a, b, |x| {
            f.sample(time::f64_to_datetime(x))
                .map(|s| (s.value, s.rate.unwrap_or(0.0)))
        })
    } else {
        refinement.brent.solve(a, b, |x| {
            f.sample(time::f64_to_datetime(x)).map(|s| s.value)
        })
    }
    .inspect_err(|e| tracing::warn!(error = %e, "failed to refine crossing"))
    .map_err(crate::Error::Roots)?;
    Ok(time::f64_to_datetime(root))
}

/// A [`Detector`] that yields the refined zero [`Crossing`]s of an
/// [`EventFunction`]. Build one with [`EventIter::builder`], or with
/// [`CrossingDetector::new`] for use in a custom [`DetectIter`].
pub struct CrossingDetector<F, S> {
    f: F,
    step: S,
    refinement: Refinement,
    prev: Option<Sample>,
}

impl<F: EventFunction, S: StepStrategy> CrossingDetector<F, S> {
    /// Create a crossing detector from its parts.
    pub fn new(f: F, step: S, refinement: Refinement) -> Self {
        Self {
            f,
            step,
            refinement,
            prev: None,
        }
    }

    /// Mutably access the root-finder configuration.
    pub fn refinement_mut(&mut self) -> &mut Refinement {
        &mut self.refinement
    }
}

impl<F: EventFunction, S: StepStrategy> Detector for CrossingDetector<F, S> {
    type Event = Crossing;

    fn next_time(&mut self, current: DateTime<Utc>) -> DateTime<Utc> {
        let sample = self.prev.as_ref().filter(|s| s.time == current);
        self.step.next_time(current, sample)
    }

    fn detect_event(&mut self, t: DateTime<Utc>) -> Result<Option<Crossing>> {
        let s = self.f.sample(t)?;
        let result = match self.prev {
            Some(p) if p.value * s.value < 0.0 => {
                let direction = if p.value < 0.0 {
                    Direction::Rising
                } else {
                    Direction::Falling
                };
                let with_rate = p.rate.is_some() && s.rate.is_some();
                refine(&mut self.f, &self.refinement, p.time, s.time, with_rate)
                    .map(|time| Some(Crossing { time, direction }))
            }
            _ => Ok(None),
        };
        self.prev = Some(s);
        result
    }
}

/// An interval over which the event function held one sign.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// Start of the window: a refined crossing, or the interval start for a
    /// leading partial window.
    pub start: DateTime<Utc>,
    /// End of the window: a refined crossing, or the interval end for a
    /// trailing partial window.
    pub end: DateTime<Utc>,
    /// Whether the function value was positive throughout the window.
    pub positive: bool,
}

/// How a [`WindowDetector`] treats a window still open at the interval end.
enum EndPolicy {
    /// Clamp the trailing window to the interval end and emit it (after
    /// checking for one final crossing between the last sample and the end).
    Clamp,
    /// On entering a positive window, immediately walk forward in `step`
    /// increments — beyond the interval end if necessary — to find and
    /// refine the window's true end. Errors with
    /// [`Error::WindowEndNotFound`] if the window does not close within
    /// `timeout`.
    ScanPastEnd { step: Duration, timeout: Duration },
}

enum FinishStage {
    CheckEnd,
    Flush,
    Done,
}

/// A [`Detector`] that pairs the zero crossings of an [`EventFunction`] into
/// [`Window`]s. Build one with [`WindowIter::builder`].
///
/// By default it partitions the interval: every instant belongs to a window
/// labelled by the function's sign, and partial windows at the interval
/// boundaries are emitted clamped (the illumination-iterator behaviour).
/// The builder's [`emit_positive_only`](WindowIterBuilder::emit_positive_only),
/// [`skip_leading_partial`](WindowIterBuilder::skip_leading_partial) and
/// [`scan_past_end`](WindowIterBuilder::scan_past_end) options select the
/// transit-iterator behaviour instead.
pub struct WindowDetector<F, S> {
    f: F,
    step: S,
    refinement: Refinement,
    positive_only: bool,
    skip_leading_partial: bool,
    end_policy: EndPolicy,
    prev: Option<Sample>,
    /// Sign of the currently open window; `None` until the first sample.
    positive: Option<bool>,
    /// Start of the currently open window; `None` when the window is
    /// suppressed (leading partial with `skip_leading_partial`).
    window_start: Option<DateTime<Utc>>,
    /// Where to resume the main scan after an inline end-walk
    /// (`ScanPastEnd` mode).
    resume: Option<DateTime<Utc>>,
    finish_stage: FinishStage,
}

impl<F: EventFunction, S: StepStrategy> WindowDetector<F, S> {
    /// Mutably access the root-finder configuration.
    pub fn refinement_mut(&mut self) -> &mut Refinement {
        &mut self.refinement
    }

    fn emit(&self, window: Option<Window>) -> Option<Window> {
        window.filter(|w| !self.positive_only || w.positive)
    }

    /// Partition-mode transition handling: on a sign change between the
    /// previous sample and `s`, refine the crossing, close the open window
    /// and open the next one.
    fn partition_transition(&mut self, s: Sample) -> Result<Option<Window>> {
        let Some(p) = self.prev.replace(s) else {
            // First sample: open the initial window at the interval start.
            self.positive = Some(s.value > 0.0);
            self.window_start = if self.skip_leading_partial {
                None
            } else {
                Some(s.time)
            };
            return Ok(None);
        };
        if (p.value > 0.0) == (s.value > 0.0) {
            return Ok(None);
        }

        // Sign change detected — find the crossing. If either sample sits
        // exactly on zero, the bracket is degenerate: the sample itself is
        // the crossing and no root-finding is needed.
        let crossing = if s.value == 0.0 {
            s.time
        } else if p.value == 0.0 {
            p.time
        } else {
            let with_rate = p.rate.is_some() && s.rate.is_some();
            refine(&mut self.f, &self.refinement, p.time, s.time, with_rate)?
        };

        let closed_positive = self.positive.expect("initialized with first sample");
        let window = self.window_start.map(|start| Window {
            start,
            end: crossing,
            positive: closed_positive,
        });
        self.window_start = Some(crossing);
        // Ground the new window's sign in the actual function value so it
        // cannot accumulate error across crossings; a sample exactly on zero
        // carries no direction, so flip the previous sign instead.
        self.positive = Some(if s.value != 0.0 {
            s.value > 0.0
        } else {
            !closed_positive
        });
        Ok(self.emit(window))
    }

    /// `ScanPastEnd`-mode detection: on a rising transition, refine the
    /// window start, then walk forward in fixed steps (beyond the scan
    /// interval if necessary) until the function goes negative, and refine
    /// the window end.
    fn detect_complete_window(
        &mut self,
        s: Sample,
        walk_step: Duration,
        timeout: Duration,
    ) -> Result<Option<Window>> {
        let Some(p) = self.prev.replace(s) else {
            // A window already open at the interval start has no detectable
            // start crossing and is skipped.
            return Ok(None);
        };
        if !(p.value < 0.0 && s.value >= 0.0) {
            return Ok(None);
        }

        let with_rate = p.rate.is_some() && s.rate.is_some();
        let start = refine(&mut self.f, &self.refinement, p.time, s.time, with_rate)?;

        // Walk forward until the function goes negative. The step is fixed:
        // an adaptive step could jump clear over the window end and the next
        // window's start, merging two windows.
        let mut t0 = start;
        let mut t1 = t0 + walk_step;
        let end = loop {
            if t1 - start > timeout {
                tracing::warn!(%start, "window end not found within {timeout}");
                return Err(Error::WindowEndNotFound { start, timeout }.into());
            }
            let w = self.f.sample(t1)?;
            if w.value < 0.0 {
                let end = refine(&mut self.f, &self.refinement, t0, t1, with_rate)?;
                // Resume the main scan from the first confirmed-outside
                // sample.
                self.prev = Some(w);
                self.resume = Some(t1);
                break end;
            }
            t0 = t1;
            t1 += walk_step;
        };

        Ok(Some(Window {
            start,
            end,
            positive: true,
        }))
    }
}

impl<F: EventFunction, S: StepStrategy> Detector for WindowDetector<F, S> {
    type Event = Window;

    fn next_time(&mut self, current: DateTime<Utc>) -> DateTime<Utc> {
        if let Some(resume) = self.resume.take() {
            return resume;
        }
        let sample = self.prev.as_ref().filter(|s| s.time == current);
        self.step.next_time(current, sample)
    }

    fn detect_event(&mut self, t: DateTime<Utc>) -> Result<Option<Window>> {
        let s = self.f.sample(t)?;
        match self.end_policy {
            EndPolicy::Clamp => self.partition_transition(s),
            EndPolicy::ScanPastEnd { step, timeout } => {
                self.detect_complete_window(s, step, timeout)
            }
        }
    }

    fn finish(&mut self, end: DateTime<Utc>) -> Result<Option<Window>> {
        if matches!(self.end_policy, EndPolicy::ScanPastEnd { .. }) {
            // Complete windows were emitted as soon as they opened; nothing
            // is left to flush.
            return Ok(None);
        }
        loop {
            match self.finish_stage {
                FinishStage::CheckEnd => {
                    self.finish_stage = FinishStage::Flush;
                    if self.positive.is_none() {
                        // Empty interval: no samples, nothing to yield.
                        self.finish_stage = FinishStage::Done;
                        return Ok(None);
                    }
                    // A crossing may still lie between the last scan sample
                    // and the exact interval end.
                    let s = self.f.sample(end)?;
                    if let Some(window) = self.partition_transition(s)? {
                        return Ok(Some(window));
                    }
                }
                FinishStage::Flush => {
                    self.finish_stage = FinishStage::Done;
                    let positive = self.positive.expect("checked in CheckEnd");
                    let window = self.window_start.map(|start| Window {
                        start,
                        end,
                        positive,
                    });
                    return Ok(self.emit(window));
                }
                FinishStage::Done => return Ok(None),
            }
        }
    }
}

/// Placeholder for a builder slot that has not been filled yet.
///
/// Builders start life as e.g. `EventIterBuilder<Missing>`; supplying the
/// event function rewrites the type parameter, and `build()` only exists
/// once every `Missing` slot is filled — a forgotten function is a compile
/// error, not a runtime one.
pub struct Missing;

/// Iterator over the zero [`Crossing`]s of an [`EventFunction`].
///
/// Create with [`EventIter::builder`].
pub type EventIter<F = Missing, S = FixedStep> = DetectIter<CrossingDetector<F, S>>;

/// Iterator over the sign [`Window`]s of an [`EventFunction`].
///
/// Create with [`WindowIter::builder`].
pub type WindowIter<F = Missing, S = FixedStep> = DetectIter<WindowDetector<F, S>>;

impl EventIter {
    /// Start building an [`EventIter`].
    pub fn builder() -> EventIterBuilder {
        EventIterBuilder::new()
    }
}

impl WindowIter {
    /// Start building a [`WindowIter`].
    pub fn builder() -> WindowIterBuilder {
        WindowIterBuilder::new()
    }
}

fn missing_interval(kind: &str) -> crate::Error {
    crate::Error::Interval(format!(
        "{kind} requires an interval — call .interval(start..end)"
    ))
}

/// Builder for [`EventIter`]. Obtain via [`EventIter::builder`].
pub struct EventIterBuilder<F = Missing, S = FixedStep> {
    interval: Option<Range<DateTime<Utc>>>,
    function: F,
    step: S,
    refinement: Refinement,
}

impl EventIterBuilder {
    fn new() -> Self {
        Self {
            interval: None,
            function: Missing,
            step: FixedStep(Duration::seconds(60)),
            refinement: Refinement::default(),
        }
    }
}

impl<F, S> EventIterBuilder<F, S> {
    /// The time interval to search (required).
    pub fn interval(mut self, interval: impl IntervalRange) -> Self {
        self.interval = Some(interval.start()..interval.end());
        self
    }

    /// The event function, as a plain value closure (required, unless
    /// [`function_with_rate`](Self::function_with_rate) or
    /// [`event_function`](Self::event_function) is used instead).
    pub fn function<F2>(self, f: F2) -> EventIterBuilder<ValueFn<F2>, S>
    where
        F2: FnMut(DateTime<Utc>) -> Result<f64>,
    {
        self.event_function(ValueFn(f))
    }

    /// The event function, as a closure returning `(value, rate)`. The rate
    /// enables Newton-Raphson refinement of crossings.
    pub fn function_with_rate<F2>(self, f: F2) -> EventIterBuilder<RateFn<F2>, S>
    where
        F2: FnMut(DateTime<Utc>) -> Result<(f64, f64)>,
    {
        self.event_function(RateFn(f))
    }

    /// The event function, as any [`EventFunction`] implementation.
    pub fn event_function<F2: EventFunction>(self, f: F2) -> EventIterBuilder<F2, S> {
        EventIterBuilder {
            interval: self.interval,
            function: f,
            step: self.step,
            refinement: self.refinement,
        }
    }

    /// The stepping strategy (default: [`FixedStep`] of 60 seconds).
    pub fn step<S2: StepStrategy>(self, step: S2) -> EventIterBuilder<F, S2> {
        EventIterBuilder {
            interval: self.interval,
            function: self.function,
            step,
            refinement: self.refinement,
        }
    }

    /// The root-finder configuration used to refine crossings.
    pub fn refinement(mut self, refinement: Refinement) -> Self {
        self.refinement = refinement;
        self
    }
}

impl<F: EventFunction, S: StepStrategy> EventIterBuilder<F, S> {
    /// Build the iterator. Errors if no interval was supplied.
    pub fn build(self) -> Result<EventIter<F, S>> {
        let interval = self.interval.ok_or_else(|| missing_interval("EventIter"))?;
        Ok(DetectIter::new(
            interval,
            CrossingDetector::new(self.function, self.step, self.refinement),
        ))
    }
}

/// Builder for [`WindowIter`]. Obtain via [`WindowIter::builder`].
pub struct WindowIterBuilder<F = Missing, S = FixedStep> {
    interval: Option<Range<DateTime<Utc>>>,
    function: F,
    step: S,
    refinement: Refinement,
    positive_only: bool,
    skip_leading_partial: bool,
    end_policy: EndPolicy,
}

impl WindowIterBuilder {
    fn new() -> Self {
        Self {
            interval: None,
            function: Missing,
            step: FixedStep(Duration::seconds(60)),
            refinement: Refinement::default(),
            positive_only: false,
            skip_leading_partial: false,
            end_policy: EndPolicy::Clamp,
        }
    }
}

impl<F, S> WindowIterBuilder<F, S> {
    /// The time interval to search (required).
    pub fn interval(mut self, interval: impl IntervalRange) -> Self {
        self.interval = Some(interval.start()..interval.end());
        self
    }

    /// The event function, as a plain value closure (required, unless
    /// [`function_with_rate`](Self::function_with_rate) or
    /// [`event_function`](Self::event_function) is used instead).
    pub fn function<F2>(self, f: F2) -> WindowIterBuilder<ValueFn<F2>, S>
    where
        F2: FnMut(DateTime<Utc>) -> Result<f64>,
    {
        self.event_function(ValueFn(f))
    }

    /// The event function, as a closure returning `(value, rate)`. The rate
    /// enables Newton-Raphson refinement of crossings.
    pub fn function_with_rate<F2>(self, f: F2) -> WindowIterBuilder<RateFn<F2>, S>
    where
        F2: FnMut(DateTime<Utc>) -> Result<(f64, f64)>,
    {
        self.event_function(RateFn(f))
    }

    /// The event function, as any [`EventFunction`] implementation.
    pub fn event_function<F2: EventFunction>(self, f: F2) -> WindowIterBuilder<F2, S> {
        WindowIterBuilder {
            interval: self.interval,
            function: f,
            step: self.step,
            refinement: self.refinement,
            positive_only: self.positive_only,
            skip_leading_partial: self.skip_leading_partial,
            end_policy: self.end_policy,
        }
    }

    /// The stepping strategy (default: [`FixedStep`] of 60 seconds).
    pub fn step<S2: StepStrategy>(self, step: S2) -> WindowIterBuilder<F, S2> {
        WindowIterBuilder {
            interval: self.interval,
            function: self.function,
            step,
            refinement: self.refinement,
            positive_only: self.positive_only,
            skip_leading_partial: self.skip_leading_partial,
            end_policy: self.end_policy,
        }
    }

    /// The root-finder configuration used to refine crossings.
    pub fn refinement(mut self, refinement: Refinement) -> Self {
        self.refinement = refinement;
        self
    }

    /// Emit only windows where the function is positive (default: emit
    /// every window, labelled by [`Window::positive`]).
    pub fn emit_positive_only(mut self) -> Self {
        self.positive_only = true;
        self
    }

    /// Do not emit a window already open at the interval start (default:
    /// emit it, clamped to the interval start).
    pub fn skip_leading_partial(mut self) -> Self {
        self.skip_leading_partial = true;
        self
    }

    /// On entering a positive window, immediately walk forward in `step`
    /// increments — beyond the interval end if necessary — to find its true
    /// end, erroring with [`Error::WindowEndNotFound`] if the window does
    /// not close within `timeout`.
    ///
    /// In this mode only complete positive windows are emitted: a window
    /// already open at the interval start has no detectable start crossing
    /// and is skipped, and [`emit_positive_only`](Self::emit_positive_only) /
    /// [`skip_leading_partial`](Self::skip_leading_partial) are implied.
    /// This is the transit-detection behaviour. The default is instead to
    /// clamp partial windows to the interval boundaries.
    pub fn scan_past_end(mut self, step: Duration, timeout: Duration) -> Self {
        self.end_policy = EndPolicy::ScanPastEnd { step, timeout };
        self
    }
}

impl<F: EventFunction, S: StepStrategy> WindowIterBuilder<F, S> {
    /// Build the iterator. Errors if no interval was supplied.
    pub fn build(self) -> Result<WindowIter<F, S>> {
        let interval = self
            .interval
            .ok_or_else(|| missing_interval("WindowIter"))?;
        Ok(DetectIter::new(
            interval,
            WindowDetector {
                f: self.function,
                step: self.step,
                refinement: self.refinement,
                positive_only: self.positive_only,
                skip_leading_partial: self.skip_leading_partial,
                end_policy: self.end_policy,
                prev: None,
                positive: None,
                window_start: None,
                resume: None,
                finish_stage: FinishStage::CheckEnd,
            },
        ))
    }
}

/// Errors that can occur during generic event detection.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error(
        "window end not found: event function remained non-negative \
         for more than {timeout} after {start}"
    )]
    WindowEndNotFound {
        start: DateTime<Utc>,
        timeout: Duration,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    /// Seconds since `t0` as f64.
    fn secs(t: DateTime<Utc>) -> f64 {
        (t - t0()).num_milliseconds() as f64 / 1e3
    }

    /// A sine wave with the given period in seconds: rising crossings at
    /// integer multiples of `period`, falling crossings at half-periods.
    fn sine(period: f64) -> impl FnMut(DateTime<Utc>) -> Result<f64> {
        move |t| Ok((secs(t) * std::f64::consts::TAU / period).sin())
    }

    // --- EventIter / CrossingDetector ---

    #[test]
    fn test_event_iter_finds_alternating_crossings() {
        // 600 s period over 1500 s: crossings at 300, 600, 900, 1200 s.
        let iter = EventIter::builder()
            .interval(t0()..t0() + Duration::seconds(1500))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .build()
            .unwrap();

        let crossings: Vec<Crossing> = iter.map(|c| c.unwrap()).collect();
        let expected = [
            (300.0, Direction::Falling),
            (600.0, Direction::Rising),
            (900.0, Direction::Falling),
            (1200.0, Direction::Rising),
        ];
        assert_eq!(crossings.len(), expected.len());
        for (crossing, (at, direction)) in crossings.iter().zip(expected) {
            assert!(
                (secs(crossing.time) - at).abs() < 1e-3,
                "crossing at {} s, expected {at} s",
                secs(crossing.time)
            );
            assert_eq!(crossing.direction, direction);
        }
    }

    #[test]
    fn test_event_iter_with_rate_uses_newton_raphson() {
        // Linear ramp f(t) = t - 500 with derivative 1: a single rising
        // crossing at 500 s, refinable by Newton-Raphson in one step.
        let iter = EventIter::builder()
            .interval(t0()..t0() + Duration::seconds(1000))
            .function_with_rate(|t| Ok((secs(t) - 500.0, 1.0)))
            .step(FixedStep(Duration::seconds(300)))
            .build()
            .unwrap();

        let crossings: Vec<Crossing> = iter.map(|c| c.unwrap()).collect();
        assert_eq!(crossings.len(), 1);
        assert!((secs(crossings[0].time) - 500.0).abs() < 1e-3);
        assert_eq!(crossings[0].direction, Direction::Rising);
    }

    #[test]
    fn test_event_iter_empty_interval_yields_nothing() {
        let iter = EventIter::builder()
            .interval(t0()..t0())
            .function(sine(600.0))
            .build()
            .unwrap();
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_event_iter_yields_error_and_continues() {
        // Fail at exactly one sample (t = 120 s); crossings elsewhere must
        // still be found.
        let mut f = sine(600.0);
        let iter = EventIter::builder()
            .interval(t0()..t0() + Duration::seconds(700))
            .function(move |t| {
                if (secs(t) - 120.0).abs() < 1e-9 {
                    Err(crate::Error::Interval("boom".into()))
                } else {
                    f(t)
                }
            })
            .step(FixedStep(Duration::seconds(60)))
            .build()
            .unwrap();

        let results: Vec<_> = iter.collect();
        let errors = results.iter().filter(|r| r.is_err()).count();
        let crossings: Vec<&Crossing> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(errors, 1);
        assert_eq!(crossings.len(), 2); // 300 s falling, 600 s rising
    }

    #[test]
    fn test_builder_missing_interval_errors() {
        assert!(EventIter::builder().function(sine(600.0)).build().is_err());
        assert!(WindowIter::builder().function(sine(600.0)).build().is_err());
    }

    // --- WindowIter / WindowDetector: partition (clamp) mode ---

    #[test]
    fn test_window_iter_partitions_interval() {
        // sine(600): positive on (0, 300), negative on (300, 600), ...
        // Over [10, 1490): windows at 10-300 (+), 300-600 (-), 600-900 (+),
        // 900-1200 (-), 1200-1490 (+, trailing partial clamped to end).
        // (Both interval bounds land mid-window: a sample exactly on a
        // crossing has a floating-point-dependent, degenerate sign.)
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(1490))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 5);
        // Contiguous partition covering the whole interval.
        assert_eq!(windows[0].start, t0() + Duration::seconds(10));
        assert_eq!(windows[4].end, t0() + Duration::seconds(1490));
        for pair in windows.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        // Alternating signs starting positive.
        for (i, w) in windows.iter().enumerate() {
            assert_eq!(w.positive, i % 2 == 0, "window {i} sign");
        }
        // Interior boundaries at multiples of 300 s.
        for (i, w) in windows.iter().enumerate().skip(1) {
            assert!(
                (secs(w.start) - 300.0 * i as f64).abs() < 1e-3,
                "window {i} starts at {} s",
                secs(w.start)
            );
        }
    }

    #[test]
    fn test_window_iter_positive_only_filters() {
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(1490))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .emit_positive_only()
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|w| w.positive));
    }

    #[test]
    fn test_window_iter_skip_leading_partial() {
        // First (partial) window 10-300 s is suppressed; the rest emit.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(700))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .skip_leading_partial()
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 2); // 300-600 (-), 600-700 (+ trailing)
        assert!((secs(windows[0].start) - 300.0).abs() < 1e-3);
        assert!(!windows[0].positive);
        assert_eq!(windows[1].end, t0() + Duration::seconds(700));
        assert!(windows[1].positive);
    }

    #[test]
    fn test_window_iter_detects_transition_before_exact_end() {
        // Interval ends at 310 s — just after the 300 s falling crossing.
        // Step 80 s puts the last scan sample at 250 s, so the crossing at
        // 300 s is only discoverable by the finish() end-check between the
        // last sample and the exact interval end.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(310))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(80)))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 2);
        assert!(windows[0].positive);
        assert!((secs(windows[0].end) - 300.0).abs() < 1e-3);
        assert!(!windows[1].positive);
        assert_eq!(windows[1].end, t0() + Duration::seconds(310));
    }

    // --- WindowIter: scan-past-end (complete windows) mode ---

    #[test]
    fn test_window_iter_scan_past_end_completes_trailing_window() {
        // Positive window (600, 900) straddles the interval end at 700 s:
        // the emitted window's end must be the true 900 s crossing, beyond
        // the interval.
        let iter = WindowIter::builder()
            .interval(t0()..t0() + Duration::seconds(700))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .scan_past_end(Duration::seconds(30), Duration::hours(1))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 1);
        assert!((secs(windows[0].start) - 600.0).abs() < 1e-3);
        assert!((secs(windows[0].end) - 900.0).abs() < 1e-3);
    }

    #[test]
    fn test_window_iter_scan_past_end_skips_open_window_at_start() {
        // The positive window (0, 300) is already open at the interval
        // start and must not be emitted; the (600, 900) window must be.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(30)..t0() + Duration::seconds(1000))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .scan_past_end(Duration::seconds(30), Duration::hours(1))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 1);
        assert!((secs(windows[0].start) - 600.0).abs() < 1e-3);
    }

    #[test]
    fn test_window_iter_scan_past_end_timeout_errors() {
        // f rises at 100 s and never comes back down: the end walk must
        // give up after the timeout.
        let iter = WindowIter::builder()
            .interval(t0()..t0() + Duration::seconds(600))
            .function(|t| Ok(secs(t) - 100.0))
            .step(FixedStep(Duration::seconds(60)))
            .scan_past_end(Duration::seconds(30), Duration::minutes(5))
            .build()
            .unwrap();

        let results: Vec<_> = iter.collect();
        assert!(matches!(
            results[0],
            Err(crate::Error::Detect(Error::WindowEndNotFound { .. }))
        ));
    }

    // --- ThresholdStep (ported from transits.rs step_size tests) ---

    fn threshold_sample(value: f64, rate: f64) -> Sample {
        Sample {
            time: t0(),
            value,
            rate: Some(rate),
        }
    }

    #[test]
    fn test_threshold_step_descending_uses_max_step() {
        // rate ≤ 0 → always the max step regardless of current value.
        let mut step = ThresholdStep::default();
        let max = t0() + step.max;
        assert_eq!(
            step.next_time(t0(), Some(&threshold_sample(0.0, -0.01))),
            max
        );
        assert_eq!(step.next_time(t0(), Some(&threshold_sample(0.0, 0.0))), max);
    }

    #[test]
    fn test_threshold_step_large_gap_clamps_to_max() {
        // Far below zero, rising slowly → formula exceeds max → clamped.
        let mut step = ThresholdStep::default();
        let s = threshold_sample((-60_f64).to_radians(), 0.0001);
        assert_eq!(step.next_time(t0(), Some(&s)), t0() + step.max);
    }

    #[test]
    fn test_threshold_step_near_zero_clamps_to_min() {
        // Just below zero, rising fast → formula under min → clamped.
        let mut step = ThresholdStep::default();
        let s = threshold_sample(-0.0001, 1.0);
        assert_eq!(step.next_time(t0(), Some(&s)), t0() + step.min);
    }

    #[test]
    fn test_threshold_step_mid_range() {
        // 3° below zero rising at 0.001 rad/s ≈ 52 s — inside (min, max).
        let mut step = ThresholdStep::default();
        let s = threshold_sample(-3_f64.to_radians(), 0.001);
        let next = step.next_time(t0(), Some(&s));
        assert!(
            next > t0() + step.min && next < t0() + step.max,
            "expected mid-range step, got {:?}",
            next - t0()
        );
    }

    #[test]
    fn test_threshold_step_without_sample_uses_max() {
        let mut step = ThresholdStep::default();
        assert_eq!(step.next_time(t0(), None), t0() + step.max);
    }
}
