use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch};
use uom::si::{f64, length::kilometer, velocity::kilometer_per_second};

pub trait Satellite: HasId + HasTle {}
impl<T> Satellite for T where T: HasId + HasTle {}

pub trait HasId {
    fn id(&self) -> String;
}

pub trait HasTle {
    fn line_1(&self) -> String;
    fn line_2(&self) -> String;
}

pub trait IntervalRange {
    fn start(&self) -> DateTime<Utc>;
    fn end(&self) -> DateTime<Utc>;
}

/// State vector in TEME frame
pub struct Prediction {
    pub position: Position,
    pub velocity: Velocity,
}

/// Position in TEME frame
pub struct Position {
    pub x: f64::Length,
    pub y: f64::Length,
    pub z: f64::Length,
}

/// Velocity in TEME frame
pub struct Velocity {
    pub x: f64::Velocity,
    pub y: f64::Velocity,
    pub z: f64::Velocity,
}

pub struct PredictionIter {
    elements: Elements,
    constants: Constants,
    interval: Interval,
    next_time: DateTime<Utc>,
    step: Duration,
}

impl From<sgp4::Prediction> for Prediction {
    fn from(value: sgp4::Prediction) -> Self {
        Self {
            position: Position {
                x: f64::Length::new::<kilometer>(value.position[0]),
                y: f64::Length::new::<kilometer>(value.position[1]),
                z: f64::Length::new::<kilometer>(value.position[2]),
            },
            velocity: Velocity {
                x: f64::Velocity::new::<kilometer_per_second>(value.velocity[0]),
                y: f64::Velocity::new::<kilometer_per_second>(value.velocity[1]),
                z: f64::Velocity::new::<kilometer_per_second>(value.velocity[2]),
            },
        }
    }
}

impl PredictionIter {
    fn new(
        elements: &Elements,
        constants: &Constants,
        interval: &impl IntervalRange,
        step: Duration,
    ) -> Self {
        Self {
            elements: elements.clone(),
            constants: constants.clone(),
            interval: Interval::new(interval.start(), interval.end()),
            next_time: interval.start(),
            step,
        }
    }
}

impl Iterator for PredictionIter {
    type Item = Result<Prediction, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_time >= self.interval.end {
            return None;
        }

        match self
            .constants
            .propagate(minutes_since_epoch(&self.elements, self.next_time))
            .map_err(Error::Sgp4Error)
        {
            Ok(prediction) => {
                self.next_time = self.next_time + self.step;
                Some(Ok(prediction.into()))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Defines a time range
pub struct Interval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Interval {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }
}

impl IntervalRange for Interval {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

/// Stores orbital elements and constants. Has methods to create iterators to propagate predictions
/// in given frames.
pub struct Predictor {
    elements: Elements,
    constants: Constants,
}

impl Predictor {
    pub fn new(sat: &impl Satellite) -> Self {
        // TODO: convert to try_new with error handling
        let elements = Elements::from_tle(
            Some(sat.id()),
            sat.line_1().as_bytes(),
            sat.line_2().as_bytes(),
        )
        .expect(&format!(
            "Failed to generate elements for sat '{}'",
            sat.id()
        ));
        let constants = Constants::from_elements(&elements).expect(&format!(
            "Failed to generate constants for sat '{}'",
            sat.id()
        ));
        Self {
            elements,
            constants,
        }
    }

    /// Propagate the TLE in the TEME frame over the interval in steps.
    ///
    /// Returns an iterator over predictions.
    pub fn propagate(&self, interval: &impl IntervalRange, step: Duration) -> PredictionIter {
        PredictionIter::new(&self.elements, &self.constants, interval, step)
    }
}

fn minutes_since_epoch(elements: &Elements, t: DateTime<Utc>) -> MinutesSinceEpoch {
    let epoch = DateTime::<Utc>::from_naive_utc_and_offset(elements.datetime, Utc);
    let duration = t.signed_duration_since(epoch);
    MinutesSinceEpoch(duration.num_seconds() as f64 / 60.0)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SGP4 error: {0}")]
    Sgp4Error(sgp4::Error),
}
