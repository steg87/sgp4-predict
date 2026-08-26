//! Time interval types and the [`DateTimeIter`] stepping iterator.

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::detect::MIN_POSITIVE_STEP;

/// A half-open time interval `[start, end)`.
///
/// Anything that spans time can implement it, and any implementor can be
/// passed directly to the prediction and observation iterators.
pub trait IntervalRange {
    /// Inclusive start of the interval.
    fn start(&self) -> DateTime<Utc>;
    /// Exclusive end of the interval.
    fn end(&self) -> DateTime<Utc>;

    /// Returns the duration of the interval.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{Duration, TimeZone, Utc};
    /// use sgp4_predict::IntervalRange;
    ///
    /// let a = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 30, 0).unwrap();
    /// assert_eq!(a.duration(), Duration::minutes(90));
    /// ```
    fn duration(&self) -> Duration {
        self.end() - self.start()
    }

    /// Returns the instant halfway between [`start`](Self::start) and
    /// [`end`](Self::end).
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use sgp4_predict::IntervalRange;
    ///
    /// let a = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 30, 0).unwrap();
    /// assert_eq!(a.mid_point(), Utc.with_ymd_and_hms(2024, 1, 1, 0, 45, 0).unwrap());
    /// ```
    fn mid_point(&self) -> DateTime<Utc> {
        self.start() + self.duration() / 2
    }

    /// Returns the overlap of this interval with `other` as a half-open range,
    /// or `None` if the two intervals do not overlap.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use sgp4_predict::IntervalRange;
    ///
    /// let a = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap();
    /// let b = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 30, 0).unwrap();
    /// let overlap = a.intersection(&b).unwrap();
    /// assert_eq!(overlap.start, Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap());
    /// assert_eq!(overlap.end,   Utc.with_ymd_and_hms(2024, 1, 1, 1,  0, 0).unwrap());
    ///
    /// // Disjoint intervals return None.
    /// let c = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 3, 0, 0).unwrap();
    /// assert!(a.intersection(&c).is_none());
    /// ```
    #[must_use = "returns the overlap; neither interval is modified"]
    fn intersection(&self, other: &impl IntervalRange) -> Option<Range<DateTime<Utc>>> {
        let start = self.start().max(other.start());
        let end = self.end().min(other.end());
        if start < end { Some(start..end) } else { None }
    }
}

impl IntervalRange for Range<DateTime<Utc>> {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

/// A concrete time window that can be rebuilt with different bounds.
///
/// [`IntervalRange`] only reads an interval, so it is implementable by
/// anything that merely *spans* time. `TimeWindow` is the narrower contract
/// for the detection results — [`Transit`], [`AoiWindow`], [`Illumination`],
/// the generic `Window` — whose payload fields (illumination state, window
/// sign, …) survive a change of bounds. Implement [`with_bounds`] and every
/// operation that returns a window comes with it.
///
/// [`Transit`]: crate::Transit
/// [`AoiWindow`]: crate::AoiWindow
/// [`Illumination`]: crate::Illumination
/// [`with_bounds`]: TimeWindow::with_bounds
pub trait TimeWindow: IntervalRange + Sized {
    /// Returns a copy of this window spanning `start..end`, leaving every
    /// other field unchanged.
    #[must_use = "returns the rebounded copy; the receiver is unchanged"]
    fn with_bounds(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self;

    /// Returns a copy of this window clamped to `interval`, or `None` if it
    /// lies entirely outside the interval.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use sgp4_predict::{TimeWindow, Transit};
    ///
    /// let transit = Transit::new(
    ///     Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
    ///     Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap(),
    /// );
    /// let window = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 1, 30, 0).unwrap();
    ///
    /// // Unlike `intersection`, this gives back a `Transit`, not a bare range.
    /// let clamped = transit.clamp_to(&window).unwrap();
    /// assert_eq!(
    ///     clamped,
    ///     Transit::new(
    ///         Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap(),
    ///         Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap(),
    ///     )
    /// );
    ///
    /// // Fully outside returns None.
    /// let disjoint = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap()
    ///     ..Utc.with_ymd_and_hms(2024, 1, 1, 3, 0, 0).unwrap();
    /// assert!(transit.clamp_to(&disjoint).is_none());
    /// ```
    #[must_use = "returns the clamped copy; the receiver is unchanged"]
    fn clamp_to(&self, interval: &impl IntervalRange) -> Option<Self> {
        self.intersection(interval)
            .map(|r| self.with_bounds(r.start, r.end))
    }
}

/// Iterator that yields equally-spaced [`DateTime<Utc>`] values over an interval.
///
/// Yields times `[start, start + step, start + 2·step, …)` up to but not
/// including `end`. Call `include_end` to append the exact end time as a final
/// sample.
///
/// Only a **non-positive** `step` is adjusted, to 1 second: it would never
/// advance and would iterate forever. Any positive step is used as given —
/// this is a sampling iterator, so sub-second steps are meaningful here even
/// though they are not for the coarse detection scans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct DateTimeIter {
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    step: Duration,
    pending_end: Option<DateTime<Utc>>,
}

impl DateTimeIter {
    /// Step through `interval` every `step`, from its start.
    pub fn new(interval: impl IntervalRange, step: Duration) -> Self {
        Self {
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            step: if step > Duration::zero() {
                step
            } else {
                MIN_POSITIVE_STEP
            },
            pending_end: None,
        }
    }

    /// Append the exact interval end time as a final sample.
    pub(crate) fn include_end(mut self) -> Self {
        self.pending_end = Some(self.interval.end);
        self
    }
}

impl Iterator for DateTimeIter {
    type Item = DateTime<Utc>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.interval.contains(&self.next_time) {
            let current = self.next_time;
            self.next_time += self.step;
            return Some(current);
        }
        self.pending_end.take()
    }
}

pub(crate) fn datetime_to_f64(dt: DateTime<Utc>) -> f64 {
    let secs = dt.timestamp() as f64;
    let nanos = dt.timestamp_subsec_nanos() as f64;
    secs + nanos * 1e-9
}

pub(crate) fn f64_to_datetime(t: f64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_nanos((t * 1e9) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_window_ordering_is_chronological() {
        // The window types derive `Ord`, so the sort order is silently
        // field-order dependent — moving a field above `start` would reorder
        // every `BTreeSet` of them with nothing else failing.
        use crate::detect::Window;
        use crate::illumination::{Illumination, IlluminationState};
        use crate::{AoiWindow, Transit};

        let t = |h| Utc.with_ymd_and_hms(2024, 1, 1, h, 0, 0).unwrap();

        assert!(Transit::new(t(0), t(1)) < Transit::new(t(2), t(3)));
        assert!(AoiWindow::new(t(0), t(1)) < AoiWindow::new(t(2), t(3)));
        // Equal starts fall through to the end, then to the payload field.
        assert!(
            Window {
                start: t(0),
                end: t(1),
                positive: false
            } < Window {
                start: t(0),
                end: t(2),
                positive: false
            }
        );
        assert!(
            Illumination {
                start: t(0),
                end: t(1),
                state: IlluminationState::Sunlit,
            } < Illumination {
                start: t(0),
                end: t(1),
                state: IlluminationState::Eclipse,
            }
        );
    }

    #[test]
    fn test_datetime_roundtrip() {
        // datetime_to_f64 followed by f64_to_datetime must preserve the value
        // to at least millisecond precision; f64 has ~7 fractional decimal
        // digits for 2024-era Unix timestamps (≈ 1.7 × 10⁹ s).
        let dt =
            Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 45).unwrap() + Duration::milliseconds(123);
        let dt2 = f64_to_datetime(datetime_to_f64(dt));
        assert!(
            (dt2 - dt).num_milliseconds().abs() < 1,
            "round-trip error: {dt2} vs {dt}"
        );
    }

    #[test]
    fn test_f64_subsecond_roundtrip() {
        // Verify that a sub-second component survives the round-trip.
        let t = 1_718_448_645.5_f64; // arbitrary timestamp with 0.5 s fractional part
        let dt = f64_to_datetime(t);
        let t2 = datetime_to_f64(dt);
        // f64 precision for a 2024 timestamp gives < 10 µs accuracy; check 1 ms.
        assert!(
            (t2 - t).abs() < 1e-3,
            "f64 subsecond round-trip error: {t2} vs {t}"
        );
    }

    #[test]
    fn test_clamp_preserves_payload_fields() {
        // The blanket clamp_to rebuilds via with_bounds, so fields other than the
        // bounds must survive it.
        use crate::{Illumination, IlluminationState};

        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let window = Illumination {
            start,
            end: start + Duration::hours(1),
            state: IlluminationState::Eclipse,
        };

        let clamped = window
            .clamp_to(&(start + Duration::minutes(30)..start + Duration::hours(2)))
            .unwrap();
        assert_eq!(
            clamped,
            Illumination {
                start: start + Duration::minutes(30),
                end: start + Duration::hours(1),
                state: IlluminationState::Eclipse,
            }
        );

        assert!(
            window
                .clamp_to(&(start + Duration::hours(3)..start + Duration::hours(4)))
                .is_none()
        );
    }

    #[test]
    fn test_datetime_iter_include_end() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = start + Duration::seconds(25);
        let step = Duration::seconds(10);

        let plain: Vec<_> = DateTimeIter::new(start..end, step).collect();
        assert_eq!(
            plain,
            vec![
                start,
                start + Duration::seconds(10),
                start + Duration::seconds(20)
            ]
        );

        let with_end: Vec<_> = DateTimeIter::new(start..end, step).include_end().collect();
        assert_eq!(
            with_end,
            vec![
                start,
                start + Duration::seconds(10),
                start + Duration::seconds(20),
                end,
            ]
        );
    }

    #[test]
    fn test_sub_second_step_is_used_as_given() {
        // Sampling at 100 ms must yield 10 samples per second, not be rounded
        // up to the 1 s floor the detection scans use.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = start + Duration::seconds(1);

        let times: Vec<_> = DateTimeIter::new(start..end, Duration::milliseconds(100)).collect();
        assert_eq!(times.len(), 10);
        assert_eq!(times[1], start + Duration::milliseconds(100));
    }

    #[test]
    fn test_non_positive_step_terminates() {
        // A zero step never advances next_time and would iterate forever, so
        // it falls back to 1 s — unlike a positive sub-second step, which is
        // used as given.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = start + Duration::seconds(3);

        for step in [Duration::zero(), Duration::seconds(-10)] {
            let times: Vec<_> = DateTimeIter::new(start..end, step).collect();
            assert_eq!(
                times,
                vec![
                    start,
                    start + Duration::seconds(1),
                    start + Duration::seconds(2)
                ],
                "step {step} did not fall back to 1s"
            );
        }
    }
}
