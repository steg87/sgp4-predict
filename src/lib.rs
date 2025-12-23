mod frames;
mod units;

use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch};
use std::ops::Range;

use frames::{EcefState, TemeState};

use crate::units::ScientificInstrument;

pub trait Satellite: HasId + HasTle {}
impl<T> Satellite for T where T: HasId + HasTle {}

pub trait Observer {
    fn latitude(&self) -> units::Angle;
    fn longitude(&self) -> units::Angle;
    fn altitude(&self) -> units::Length;

    fn to_ecef(&self) -> EcefState {
        let h = self.altitude().to_si();
        let a = 6378137.0; // meters
        let f = 1.0 / 298.257223563;
        let e2 = f * (2.0 - f);

        let sin_lat = self.latitude().to_si().sin();
        let cos_lat = self.latitude().to_si().cos();
        let sin_lon = self.longitude().to_si().sin();
        let cos_lon = self.longitude().to_si().cos();

        let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

        EcefState::new(
            Position::from_si(
                (n + h) * cos_lat * cos_lon,
                (n + h) * cos_lat * sin_lon,
                (n * (1.0 - e2) + h) * sin_lat,
            ),
            Velocity::default(),
        )
    }
}

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

/// State vector, takes frame as generic
#[derive(Debug, Clone, Copy, Default)]
pub struct StateVector<F> {
    pub position: Position,
    pub velocity: Velocity,
    _frame: std::marker::PhantomData<F>,
}

impl<F> StateVector<F> {
    pub fn new(position: Position, velocity: Velocity) -> Self {
        Self {
            position,
            velocity,
            _frame: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Position {
    pub x: units::Length,
    pub y: units::Length,
    pub z: units::Length,
}

impl std::ops::Sub for Position {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_si(
            self.x.to_si() - rhs.x.to_si(),
            self.y.to_si() - rhs.y.to_si(),
            self.z.to_si() - rhs.z.to_si(),
        )
    }
}

/// Velocity vector
#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity {
    pub x: units::Velocity,
    pub y: units::Velocity,
    pub z: units::Velocity,
}

impl std::ops::Sub for Velocity {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_si(
            self.x.to_si() - rhs.x.to_si(),
            self.y.to_si() - rhs.y.to_si(),
            self.z.to_si() - rhs.z.to_si(),
        )
    }
}

pub struct PredictionIter {
    predictor: Predictor,
    dt_iter: DateTimeIter,
}

impl From<sgp4::Prediction> for TemeState {
    fn from(value: sgp4::Prediction) -> Self {
        Self {
            // Convert sgp4::Prediction.position units (km) to SI (m)
            position: Position::from_si(
                value.position[0] * 1e3,
                value.position[1] * 1e3,
                value.position[2] * 1e3,
            ),
            // Convert sgp4::Prediction.velocity units (km/s) to SI (m/s)
            velocity: Velocity::from_si(
                value.velocity[0] * 1e3,
                value.velocity[1] * 1e3,
                value.velocity[2] * 1e3,
            ),
            _frame: std::marker::PhantomData,
        }
    }
}

impl PredictionIter {
    fn new(predictor: Predictor, interval: impl IntervalRange, step: Duration) -> Self {
        Self {
            predictor,
            dt_iter: DateTimeIter::new(interval, step),
        }
    }
}

impl Iterator for PredictionIter {
    type Item = Result<(DateTime<Utc>, TemeState), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let current_time = self.dt_iter.next()?;

        match self.predictor.propagate(current_time) {
            Ok(prediction) => Some(Ok((current_time, prediction))),
            Err(e) => Some(Err(e)),
        }
    }
}

pub struct Observation {
    pub azimuth: units::Angle,
    pub elevation: units::Angle,
    pub range: units::Length,
}

pub struct ObservationIter<'a, O: Observer> {
    predict_iter: PredictionIter,
    observer: &'a O,
}

impl<'a, O: Observer> ObservationIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: impl IntervalRange,
        step: Duration,
    ) -> Self {
        Self {
            predict_iter: PredictionIter::new(predictor, interval, step),
            observer,
        }
    }
}

impl<'a, O: Observer> Iterator for ObservationIter<'a, O> {
    type Item = Result<(DateTime<Utc>, Observation), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let (time, teme_state) = self.predict_iter.next()?.ok()?;
        Some(Ok((
            time,
            teme_state
                .to_ecef(time)
                .to_enu(self.observer)
                .to_observation(),
        )))
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

pub struct DateTimeIter {
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    step: Duration,
}

impl DateTimeIter {
    fn new(interval: impl IntervalRange, step: Duration) -> Self {
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

/// Stores orbital elements and constants. Has methods to create iterators to propagate predictions
/// in given frames.
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
}

impl Predictor {
    pub fn new(sat: &impl Satellite) -> Self {
        let elements = Elements::from_tle(
            Some(sat.id()),
            sat.line_1().as_bytes(),
            sat.line_2().as_bytes(),
        )
        .expect("Failed to generate elements for sat");
        let constants =
            Constants::from_elements(&elements).expect("Failed to generate constants for sat");
        Self {
            elements,
            constants,
        }
    }

    /// Propagate the TLE to given time t.
    ///
    /// Returns a predicted state vector in the TEME frame.
    pub fn propagate(&self, t: DateTime<Utc>) -> Result<TemeState, Error> {
        let minutes_since_epoch =
            MinutesSinceEpoch(self.time_since_epoch(t).num_milliseconds() as f64 / 60e3);
        let prediction = self
            .constants
            .propagate(minutes_since_epoch)
            .map_err(Error::Sgp4)?;
        Ok(prediction.into())
    }

    /// Propagate the TLE over a time interval.
    ///
    /// Returns an iterator over predicted state vectors in the TEME frame.
    pub fn prediction_iter(&self, interval: impl IntervalRange, step: Duration) -> PredictionIter {
        PredictionIter::new(self.clone(), interval, step)
    }

    /// Observe the TLE from an observer on Earth.
    ///
    /// Returns an iterator over observations.
    pub fn observation_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        step: Duration,
    ) -> ObservationIter<'a, O> {
        ObservationIter::new(self.clone(), observer, interval, step)
    }

    /// Calculate the number of minutes since the predictor epoch
    pub fn time_since_epoch(&self, t: DateTime<Utc>) -> Duration {
        let epoch = DateTime::<Utc>::from_naive_utc_and_offset(self.elements.datetime, Utc);
        t.signed_duration_since(epoch)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SGP4 error: {0}")]
    Sgp4(sgp4::Error),
    #[error("Interval error: {0}")]
    Interval(String),
}
