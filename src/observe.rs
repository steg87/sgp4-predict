use chrono::{DateTime, Duration, Utc};

use crate::{
    Error, Predictor,
    frames::EcefState,
    predict::PredictionIter,
    time::IntervalRange,
    units,
    vectors::{Position, Velocity},
};

pub trait Observer {
    fn latitude(&self) -> units::Angle;
    fn longitude(&self) -> units::Angle;
    fn altitude(&self) -> units::Length;

    fn to_ecef(&self) -> EcefState {
        let h = self.altitude();
        let a = 6378137.0; // meters
        let f = 1.0 / 298.257223563;
        let e2 = f * (2.0 - f);

        let sin_lat = self.latitude().sin();
        let cos_lat = self.latitude().cos();
        let sin_lon = self.longitude().sin();
        let cos_lon = self.longitude().cos();

        let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

        EcefState::new(
            Position {
                x: (n + h) * cos_lat * cos_lon,
                y: (n + h) * cos_lat * sin_lon,
                z: (n * (1.0 - e2) + h) * sin_lat,
            },
            Velocity::default(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub azimuth: units::Angle,
    pub elevation: units::Angle,
    pub range: units::Length,
    pub range_rate: units::Velocity,
}

pub struct ObservationIter<'a, O: Observer> {
    predict_iter: PredictionIter,
    observer: &'a O,
}

impl<'a, O: Observer> ObservationIter<'a, O> {
    pub fn new(
        predictor: Predictor,
        observer: &'a O,
        interval: &impl IntervalRange,
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
