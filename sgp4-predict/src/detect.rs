//! Generic event and window detection over scalar functions of time.
//!
//! This module contains the building blocks that power [`ApsisIter`],
//! [`TransitIter`] and [`IlluminationIter`], re-exported at the crate root
//! when the `generics` Cargo feature is enabled so that new kinds of
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
//! Crossings bracketed by two samples are refined with the crate's
//! bracketed hybrid solver ([`Refinement`](crate::Refinement)): each
//! iteration takes a Newton-Raphson step when its sample carries a
//! derivative ([`Sample::rate`]) and a secant/bisection step otherwise,
//! converging when the crossing is pinned down to the solver's time
//! tolerance.
//!
//! # Example: northward equator crossings
//!
//! In the TEME frame the equator is the plane `z = 0`, so ascending-node
//! crossings are the rising zero crossings of the satellite's z coordinate:
//!
#![cfg_attr(feature = "generics", doc = "```no_run")]
#![cfg_attr(not(feature = "generics"), doc = "```ignore")]
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

// Without the `generics` feature, items that exist only for external use
// (builder adapters, accessors) are not reachable from outside the crate.
#![cfg_attr(not(feature = "generics"), allow(dead_code))]

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
    /// Refinement takes a fast Newton-Raphson step at samples that carry a
    /// rate and a secant/bisection step at samples that don't; adaptive
    /// step strategies ([`ThresholdStep`]) also use it.
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
                // Clamping the f64 seconds to at most max_seconds first
                // keeps the Duration::seconds conversion below from
                // overflowing, and already bounds the result to self.max —
                // only the floor still needs enforcing.
                let seconds = (-value / rate).clamp(0.0, self.max.num_seconds() as f64);
                Duration::seconds(seconds as i64).max(self.min)
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

/// Adapts an [`EventFunction`] bracket to [`Refinement::solve`], which does
/// the actual root finding: converts the `DateTime` bracket to the solver's
/// f64-seconds domain (and the root back) and reshapes samples into
/// `(value, rate)` pairs.
///
/// Shared by every crossing refinement in this module.
fn refine_crossing<F: EventFunction>(
    f: &mut F,
    refinement: &Refinement,
    t0: DateTime<Utc>,
    t1: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let a = time::datetime_to_f64(t0);
    let b = time::datetime_to_f64(t1);
    let root = refinement
        .solve(a, b, |x| {
            f.sample(time::f64_to_datetime(x))
                .map(|s| (s.value, s.rate))
        })
        .inspect_err(|e| tracing::warn!(error = %e, "failed to refine crossing"))?;
    Ok(time::f64_to_datetime(root))
}

/// Walk from `t0` (a sample already known to be non-negative) in `step`
/// increments — negative `step` walks backward — until a negative sample is
/// found, then refines that bracket to the crossing.
///
/// Returns `(refined, resume_from)`: `refined` is the precise crossing (or,
/// if clamped, the clamp bound), suitable for reporting as a window
/// boundary. `resume_from` is always an already-*sampled* point — `refined`
/// itself is not, in general (its exact value only ever comes from bisecting
/// down to a tolerance, not from an `EventFunction` evaluation) — so it must
/// never be re-sampled to decide what lies past it: at the true crossing,
/// sign is inherently ambiguous in floating point, and resuming a scan from
/// an ambiguous point risks detecting the same crossing over and over,
/// forever.
///
/// If `clamp` is `Some(bound)` and the walk would step at or past `bound`
/// before finding a negative sample, `bound` itself is sampled — there may
/// be a crossing between the previous sample and `bound` that a larger step
/// would otherwise skip — and used as both the refined and resume value if
/// it isn't negative either. Otherwise, the walk gives up once it passes
/// `giving_up_at` — an absolute time, not a duration, so the caller can
/// bound the two directions of a window search by its *total* size rather
/// than by how far each one individually strays from the query point —
/// calling `on_give_up` to produce the error.
fn walk_to_crossing<F: EventFunction>(
    f: &mut F,
    t0: DateTime<Utc>,
    step: Duration,
    giving_up_at: DateTime<Utc>,
    clamp: Option<DateTime<Utc>>,
    refinement: &Refinement,
    on_give_up: impl FnOnce() -> Error,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let past = |t: DateTime<Utc>, bound: DateTime<Utc>| {
        (step < Duration::zero() && t <= bound) || (step > Duration::zero() && t >= bound)
    };
    let mut prev = t0;
    let mut candidate = t0 + step;
    loop {
        let clamped_at = clamp.filter(|&bound| past(candidate, bound));
        let next = clamped_at.unwrap_or(candidate);

        if clamped_at.is_none() && past(next, giving_up_at) {
            tracing::warn!(at = %t0, %giving_up_at, "window boundary not found");
            return Err(on_give_up().into());
        }

        if f.sample(next)?.value < 0.0 {
            let refined = refine_crossing(f, refinement, prev, next)?;
            return Ok((refined, next));
        }
        if let Some(bound) = clamped_at {
            return Ok((bound, bound));
        }
        prev = next;
        candidate = next + step;
    }
}

/// Walk outward from `t` to find the positive window containing it.
///
/// Returns `Ok(None)` if `t` does not currently lie inside a positive
/// window. Otherwise walks backward from `t` in `step` increments until a
/// negative sample is found, refining that bracket to the window's start,
/// then walks forward the same way to find its end. Neither walk is bounded
/// by an interval, so a window can be resolved even when it extends
/// arbitrarily far from `t` — only its total duration is bounded, by
/// `max_window_duration`; [`Error::WindowTooLong`] if it's exceeded.
///
/// This is the primitive [`WindowIter`]'s internal detector is built on; use
/// it directly for one-off "is an event in progress right now, and if so
/// what are its bounds" queries (see [`Predictor::detect_transit`] for an
/// example) rather than scanning a whole interval for one instant.
///
/// [`Predictor::detect_transit`]: crate::Predictor::detect_transit
pub fn detect_window<F: EventFunction>(
    f: &mut F,
    t: DateTime<Utc>,
    step: Duration,
    max_window_duration: Duration,
    refinement: &Refinement,
) -> Result<Option<Window>> {
    if f.sample(t)?.value < 0.0 {
        return Ok(None);
    }
    let (window, _) =
        resolve_positive_window(f, t, step, max_window_duration, None, None, refinement)?;
    Ok(Some(window))
}

/// Resolve the full positive window containing `t`, which must already be
/// known non-negative (callers that haven't just sampled `t` themselves
/// should use [`detect_window`] instead, which checks first).
///
/// Shared by [`detect_window`] and [`WindowDetector`]'s internal window
/// resolution — the only difference between the two is that the latter also
/// clamps to an enclosing interval. Also returns the point immediately past
/// the window's end that's safe to resume scanning from (see
/// [`walk_to_crossing`]'s doc comment).
fn resolve_positive_window<F: EventFunction>(
    f: &mut F,
    t: DateTime<Utc>,
    step: Duration,
    max_window_duration: Duration,
    start_clamp: Option<DateTime<Utc>>,
    end_clamp: Option<DateTime<Utc>>,
    refinement: &Refinement,
) -> Result<(Window, DateTime<Utc>)> {
    let too_long = || Error::WindowTooLong {
        at: t,
        max_window_duration,
    };
    let (start, _) = walk_to_crossing(
        f,
        t,
        -step,
        t - max_window_duration,
        start_clamp,
        refinement,
        too_long,
    )?;
    let (end, resume_from) = walk_to_crossing(
        f,
        t,
        step,
        start + max_window_duration,
        end_clamp,
        refinement,
        too_long,
    )?;
    Ok((
        Window {
            start,
            end,
            positive: true,
        },
        resume_from,
    ))
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
                refine_crossing(&mut self.f, &self.refinement, p.time, s.time)
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

/// A [`Detector`] that pairs the zero crossings of an [`EventFunction`] into
/// [`Window`]s. Build one with [`WindowIter::builder`].
///
/// The coarse scan (`S`, e.g. [`FixedStep`] or [`ThresholdStep`]) only has to
/// find *some* sample inside the next positive window; once one is found,
/// `detect_window` takes over and walks outward from it with a small fixed step
/// to pin down the window's true start and end.
///
/// A window is only considered to fall within the interval if
/// `interval.start <= window.start < interval.end`, so by default a window
/// already open at the very first sample — which cannot satisfy that — is
/// skipped ([`include_leading_partial`][ilp] opts back in), while a window
/// still open at the interval end is walked outward past it to find its true
/// end regardless ([`clamp_to_interval`][cti] opts into clamping both cases
/// to the interval bounds instead — the illumination-iterator behaviour,
/// where every instant must belong to some window).
///
/// By default only positive windows are emitted; enable
/// [`include_negative_windows`][inw] to also get the windows in between
/// (illumination needs both sunlit and eclipse windows).
///
/// [cti]: WindowIterBuilder::clamp_to_interval
/// [ilp]: WindowIterBuilder::include_leading_partial
/// [inw]: WindowIterBuilder::include_negative_windows
pub struct WindowDetector<F, S> {
    f: F,
    step: S,
    refinement: Refinement,
    positive_only: bool,
    skip_leading_partial: bool,
    clamp_to_interval: bool,
    walk_step: Duration,
    max_window_duration: Duration,
    interval: Range<DateTime<Utc>>,
    /// Last coarse-scan sample, used only to drive the `S` step strategy.
    prev: Option<Sample>,
    /// End of the most recently resolved window (or the interval start, if
    /// none yet) — the start of the next negative window, when emitted.
    last_boundary: Option<DateTime<Utc>>,
    /// A positive window found while emitting the negative window ahead of
    /// it, held back for the following call.
    pending: Option<Window>,
    /// Where to resume the coarse scan once a window has been fully
    /// resolved (its end, which may lie outside the interval).
    resume: Option<DateTime<Utc>>,
    /// Whether [`finish`](Detector::finish) has already produced its one
    /// possible trailing window.
    flushed: bool,
}

impl<F: EventFunction, S: StepStrategy> WindowDetector<F, S> {
    /// Mutably access the root-finder configuration.
    pub fn refinement_mut(&mut self) -> &mut Refinement {
        &mut self.refinement
    }

    /// The very first sample already lies inside a positive window whose
    /// start will be discarded ([`skip_leading_partial`](Self::skip_leading_partial)):
    /// walk forward only to find where it ends, without wasting a backward
    /// walk on a start nobody wants.
    fn skip_leading_window(&mut self, t: DateTime<Utc>) -> Result<Option<Window>> {
        let end_clamp = self.clamp_to_interval.then_some(self.interval.end);
        let too_long = || Error::WindowTooLong {
            at: t,
            max_window_duration: self.max_window_duration,
        };
        let (end, resume_from) = walk_to_crossing(
            &mut self.f,
            t,
            self.walk_step,
            t + self.max_window_duration,
            end_clamp,
            &self.refinement,
            too_long,
        )?;
        // Resume from an already-sampled point, never from the window's end
        // itself — see walk_to_crossing's doc comment.
        self.resume = Some(resume_from);
        self.last_boundary = Some(end);
        Ok(None)
    }

    /// Resolve the positive window found at `t` (`s.value >= 0.0`), emitting
    /// the negative window ahead of it (unless this is the first window, or
    /// `positive_only` is set) and stashing the positive window as
    /// [`pending`](Self::pending) in that case.
    fn resolve_window(&mut self, t: DateTime<Utc>, first: bool) -> Result<Option<Window>> {
        let start_clamp = self.clamp_to_interval.then_some(self.interval.start);
        let end_clamp = self.clamp_to_interval.then_some(self.interval.end);
        let (window, resume_from) = resolve_positive_window(
            &mut self.f,
            t,
            self.walk_step,
            self.max_window_duration,
            start_clamp,
            end_clamp,
            &self.refinement,
        )?;
        // Resume from an already-sampled point, never from the window's end
        // itself — see walk_to_crossing's doc comment.
        self.resume = Some(resume_from);

        let gap_start = self.last_boundary;
        self.last_boundary = Some(window.end);
        if first || self.positive_only {
            return Ok(Some(window));
        }
        self.pending = Some(window);
        Ok(Some(Window {
            start: gap_start.expect("set on every call once past the first"),
            end: window.start,
            positive: false,
        }))
    }
}

impl<F: EventFunction, S: StepStrategy> Detector for WindowDetector<F, S> {
    type Event = Window;

    fn next_time(&mut self, current: DateTime<Utc>) -> DateTime<Utc> {
        if self.pending.is_some() {
            // Stay put; detect_event will drain the pending window without
            // taking a new sample.
            return current;
        }
        if let Some(resume) = self.resume.take() {
            return resume;
        }
        let sample = self.prev.as_ref().filter(|s| s.time == current);
        self.step.next_time(current, sample)
    }

    fn detect_event(&mut self, t: DateTime<Utc>) -> Result<Option<Window>> {
        if let Some(w) = self.pending.take() {
            return Ok(Some(w));
        }
        let first = self.last_boundary.is_none();
        let s = self.f.sample(t)?;
        if s.value < 0.0 {
            if first {
                self.last_boundary = Some(t);
            }
            self.prev = Some(s);
            return Ok(None);
        }
        if first && self.skip_leading_partial {
            return self.skip_leading_window(t);
        }
        self.resolve_window(t, first)
    }

    fn finish(&mut self, end: DateTime<Utc>) -> Result<Option<Window>> {
        if let Some(w) = self.pending.take() {
            return Ok(Some(w));
        }
        if self.flushed {
            return Ok(None);
        }
        self.flushed = true;
        let Some(last_boundary) = self.last_boundary else {
            // Empty interval: no samples, nothing to yield.
            return Ok(None);
        };
        if last_boundary >= end {
            // The most recently resolved positive window already reached
            // (or, walked/clamped, extended past) the interval end: nothing
            // left to check. Sampling at `end` here would re-enter that
            // same already-emitted window and duplicate it.
            return Ok(None);
        }
        // A crossing may still lie between the last coarse-scan sample and
        // the exact interval end.
        let s = self.f.sample(end)?;
        if s.value < 0.0 {
            return Ok(
                (!self.positive_only && last_boundary < end).then_some(Window {
                    start: last_boundary,
                    end,
                    positive: false,
                }),
            );
        }
        self.resolve_window(end, false)
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

/// Default fixed step used to walk outward from a detected window to pin
/// down its exact start and end.
const DEFAULT_WALK_STEP: Duration = Duration::seconds(30);
/// Default cap on a positive window's total duration, from start to end,
/// before [`WindowDetector`] gives up with [`Error::WindowTooLong`].
const DEFAULT_MAX_WINDOW_DURATION: Duration = Duration::hours(1);

/// Builder for [`WindowIter`]. Obtain via [`WindowIter::builder`].
pub struct WindowIterBuilder<F = Missing, S = FixedStep> {
    interval: Option<Range<DateTime<Utc>>>,
    function: F,
    step: S,
    refinement: Refinement,
    positive_only: bool,
    skip_leading_partial: bool,
    clamp_to_interval: bool,
    walk_step: Duration,
    max_window_duration: Duration,
}

impl WindowIterBuilder {
    fn new() -> Self {
        Self {
            interval: None,
            function: Missing,
            step: FixedStep(Duration::seconds(60)),
            refinement: Refinement::default(),
            positive_only: true,
            skip_leading_partial: true,
            clamp_to_interval: false,
            walk_step: DEFAULT_WALK_STEP,
            max_window_duration: DEFAULT_MAX_WINDOW_DURATION,
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
            clamp_to_interval: self.clamp_to_interval,
            walk_step: self.walk_step,
            max_window_duration: self.max_window_duration,
        }
    }

    /// The coarse stepping strategy used to search for the next window
    /// (default: [`FixedStep`] of 60 seconds). Only needs to land *some*
    /// sample inside the next positive window — its exact bounds are then
    /// found by the boundary walk (see [`walk_step`](Self::walk_step)).
    pub fn step<S2: StepStrategy>(self, step: S2) -> WindowIterBuilder<F, S2> {
        WindowIterBuilder {
            interval: self.interval,
            function: self.function,
            step,
            refinement: self.refinement,
            positive_only: self.positive_only,
            skip_leading_partial: self.skip_leading_partial,
            clamp_to_interval: self.clamp_to_interval,
            walk_step: self.walk_step,
            max_window_duration: self.max_window_duration,
        }
    }

    /// The root-finder configuration used to refine crossings.
    pub fn refinement(mut self, refinement: Refinement) -> Self {
        self.refinement = refinement;
        self
    }

    /// Also emit the windows in between positive ones (default: only
    /// positive windows, labelled [`Window::positive`], are emitted).
    pub fn include_negative_windows(mut self) -> Self {
        self.positive_only = false;
        self
    }

    /// Also emit a window already open at the interval start (default:
    /// skip it — a window is usually considered to fall within an interval
    /// only if `interval.start <= window.start < interval.end`, which a
    /// window already open at the very first sample cannot satisfy). Its
    /// start is found the same way as any other boundary — see
    /// [`clamp_to_interval`](Self::clamp_to_interval).
    pub fn include_leading_partial(mut self) -> Self {
        self.skip_leading_partial = false;
        self
    }

    /// Clamp windows already open at the interval start, or still open at
    /// the interval end, to the interval bounds instead of walking outward
    /// past them to find the true boundary (the illumination-iterator
    /// behaviour: every instant belongs to a window, none of which extend
    /// beyond the requested interval).
    pub fn clamp_to_interval(mut self) -> Self {
        self.clamp_to_interval = true;
        self
    }

    /// The fixed step used to walk outward from a detected window to pin
    /// down its exact start and end (default: 30 seconds). Deliberately
    /// independent of the coarse [`step`](Self::step) strategy, which can
    /// jump clear over a short window if used for this instead.
    pub fn walk_step(mut self, step: Duration) -> Self {
        self.walk_step = step;
        self
    }

    /// The longest a positive window's total duration (start to end) is
    /// allowed to be; exceeding it gives up with [`Error::WindowTooLong`]
    /// (default: 1 hour). Only positive windows are walked out to their
    /// boundaries, so this has no effect on negative ones.
    pub fn max_window_duration(mut self, max_window_duration: Duration) -> Self {
        self.max_window_duration = max_window_duration;
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
            interval.clone(),
            WindowDetector {
                f: self.function,
                step: self.step,
                refinement: self.refinement,
                positive_only: self.positive_only,
                skip_leading_partial: self.skip_leading_partial,
                clamp_to_interval: self.clamp_to_interval,
                walk_step: self.walk_step,
                max_window_duration: self.max_window_duration,
                interval,
                prev: None,
                last_boundary: None,
                pending: None,
                resume: None,
                flushed: false,
            },
        ))
    }
}

/// Errors that can occur during generic event detection.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error(
        "window too long: the positive window containing {at} exceeds the \
         {max_window_duration} maximum"
    )]
    WindowTooLong {
        at: DateTime<Utc>,
        max_window_duration: Duration,
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

    // --- WindowIter / WindowDetector: clamp_to_interval (partition) mode ---

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
            .include_negative_windows()
            .include_leading_partial()
            .clamp_to_interval()
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
    fn test_window_iter_positive_only_is_default() {
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(1490))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .include_leading_partial()
            .clamp_to_interval()
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|w| w.positive));
    }

    #[test]
    fn test_window_iter_skip_leading_partial_is_default_even_when_clamped() {
        // First (partial) window 10-300 s is suppressed by default even in
        // clamp_to_interval mode; the rest emit.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(10)..t0() + Duration::seconds(700))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .include_negative_windows()
            .clamp_to_interval()
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
            .include_negative_windows()
            .include_leading_partial()
            .clamp_to_interval()
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 2);
        assert!(windows[0].positive);
        assert!((secs(windows[0].end) - 300.0).abs() < 1e-3);
        assert!(!windows[1].positive);
        assert_eq!(windows[1].end, t0() + Duration::seconds(310));
    }

    // --- WindowIter: default mode (walk past interval bounds) ---

    #[test]
    fn test_window_iter_completes_trailing_window_by_default() {
        // Positive window (600, 900) straddles the interval end at 700 s:
        // the emitted window's end must be the true 900 s crossing, beyond
        // the interval — the default behaviour, no builder option needed.
        let iter = WindowIter::builder()
            .interval(t0()..t0() + Duration::seconds(700))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 1);
        assert!((secs(windows[0].start) - 600.0).abs() < 1e-3);
        assert!((secs(windows[0].end) - 900.0).abs() < 1e-3);
    }

    #[test]
    fn test_window_iter_include_leading_partial_walks_past_start() {
        // Window (0, 300) is already open at the interval start; by default
        // it would be skipped (skip_leading_partial is the default), but
        // with include_leading_partial its true start is found by walking
        // backward past the interval boundary, exactly as the end is walked
        // forward past the boundary in the test above.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(30)..t0() + Duration::seconds(200))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .include_leading_partial()
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 1);
        assert!(
            secs(windows[0].start).abs() < 1e-3,
            "expected true start ≈0 s, got {}",
            secs(windows[0].start)
        );
        assert!((secs(windows[0].end) - 300.0).abs() < 1e-3);
    }

    #[test]
    fn test_window_iter_skips_open_window_at_start_by_default() {
        // The positive window (0, 300) is already open at the interval
        // start and must not be emitted; the (600, 900) window must be.
        let iter = WindowIter::builder()
            .interval(t0() + Duration::seconds(30)..t0() + Duration::seconds(1000))
            .function(sine(600.0))
            .step(FixedStep(Duration::seconds(60)))
            .build()
            .unwrap();

        let windows: Vec<Window> = iter.map(|w| w.unwrap()).collect();
        assert_eq!(windows.len(), 1);
        assert!((secs(windows[0].start) - 600.0).abs() < 1e-3);
    }

    #[test]
    fn test_window_iter_timeout_errors() {
        // f rises at 100 s and never comes back down: the end walk must
        // give up after the max_window_duration.
        let iter = WindowIter::builder()
            .interval(t0()..t0() + Duration::seconds(600))
            .function(|t| Ok(secs(t) - 100.0))
            .step(FixedStep(Duration::seconds(60)))
            .max_window_duration(Duration::minutes(5))
            .build()
            .unwrap();

        let results: Vec<_> = iter.collect();
        assert!(matches!(
            results[0],
            Err(crate::Error::Detect(Error::WindowTooLong { .. }))
        ));
    }

    // --- detect_window ---

    #[test]
    fn test_detect_window_finds_containing_window() {
        let mut f = ValueFn(sine(600.0));
        let t = t0() + Duration::seconds(750); // inside (600, 900)
        let window = detect_window(
            &mut f,
            t,
            Duration::seconds(30),
            Duration::hours(1),
            &Refinement::default(),
        )
        .unwrap()
        .unwrap();

        assert!((secs(window.start) - 600.0).abs() < 1e-3);
        assert!((secs(window.end) - 900.0).abs() < 1e-3);
        assert!(window.positive);
    }

    #[test]
    fn test_detect_window_returns_none_outside_window() {
        let mut f = ValueFn(sine(600.0));
        let t = t0() + Duration::seconds(450); // inside negative (300, 600)
        let window = detect_window(
            &mut f,
            t,
            Duration::seconds(30),
            Duration::hours(1),
            &Refinement::default(),
        )
        .unwrap();

        assert!(window.is_none());
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

    #[test]
    fn test_threshold_step_sub_second_min_still_advances() {
        // A sub-second min must not truncate to a zero step and stall.
        let mut step = ThresholdStep {
            min: Duration::milliseconds(500),
            max: Duration::minutes(10),
        };
        let s = threshold_sample(-0.0001, 1.0);
        assert_eq!(step.next_time(t0(), Some(&s)), t0() + step.min);
    }

    #[test]
    fn test_threshold_step_nan_value_still_advances() {
        // A NaN ratio must land on min, not stall at a zero step.
        let mut step = ThresholdStep::default();
        let s = threshold_sample(f64::NAN, 1.0);
        assert_eq!(step.next_time(t0(), Some(&s)), t0() + step.min);
    }
}
