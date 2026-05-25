//! Time interval types and the [`DateTimeIter`] stepping iterator.

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

/// A half-open time interval `[start, end)`.
///
/// Implemented for `Range<DateTime<Utc>>` and for [`Transit`] and
/// [`Illumination`], so either can be passed directly to the prediction and
/// observation iterators.
///
/// [`Transit`]: crate::Transit
/// [`Illumination`]: crate::Illumination
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

/// Iterator that yields equally-spaced [`DateTime<Utc>`] values over an interval.
///
/// Yields times `[start, start + step, start + 2·step, …)` up to but not
/// including `end`. Call [`include_end`](DateTimeIter::include_end) to append
/// the exact end time as a final sample.
pub struct DateTimeIter {
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    step: Duration,
    pending_end: Option<DateTime<Utc>>,
}

impl DateTimeIter {
    pub fn new(interval: impl IntervalRange, step: Duration) -> Self {
        Self {
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            step,
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
}
