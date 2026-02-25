use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

pub trait IntervalRange {
    fn start(&self) -> DateTime<Utc>;
    fn end(&self) -> DateTime<Utc>;
}

impl IntervalRange for Range<DateTime<Utc>> {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

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
