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
}

impl DateTimeIter {
    pub fn new(interval: &impl IntervalRange, step: Duration) -> Self {
        Self {
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            step,
        }
    }
}

impl Iterator for DateTimeIter {
    type Item = DateTime<Utc>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.interval.contains(&self.next_time) {
            return None;
        }
        let current = self.next_time;
        self.next_time += self.step;
        Some(current)
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
