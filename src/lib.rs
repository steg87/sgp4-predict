mod apsides;
mod frames;
mod illumination;
mod observe;
mod predict;
mod roots;
mod time;
mod transits;
mod vectors;

use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch};
use thiserror::Error as ThisError;

pub use crate::{
    apsides::{Apsis, ApsisEvent, ApsisIter},
    frames::TemeState,
    illumination::{Illumination, IlluminationIter, IlluminationState},
    observe::{Observation, ObservationIter, Observer},
    predict::PredictionIter,
    time::{DateTimeIter, IntervalRange},
    transits::{Transit, TransitIter},
    vectors::{Position, StateVector, Velocity},
};

pub type Result<T> = std::result::Result<T, Error>;

pub trait Satellite: HasId + HasTle {}
impl<T> Satellite for T where T: HasId + HasTle {}

pub trait HasId {
    fn id(&self) -> &str;
}

pub trait HasTle {
    fn line_1(&self) -> &str;
    fn line_2(&self) -> &str;
}

/// Stores orbital elements and constants. Has methods to create iterators to propagate predictions
/// in given frames.
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
}

impl Predictor {
    pub fn new(sat: &impl Satellite) -> Result<Self> {
        let elements = Elements::from_tle(
            Some(sat.id().to_owned()),
            sat.line_1().as_bytes(),
            sat.line_2().as_bytes(),
        )?;
        let constants = Constants::from_elements(&elements)?;
        Ok(Self {
            elements,
            constants,
        })
    }

    /// Propagate the TLE to given time t.
    ///
    /// Returns a predicted state vector in the TEME frame.
    pub fn propagate(&self, t: DateTime<Utc>) -> Result<TemeState> {
        let minutes_since_epoch = MinutesSinceEpoch(
            t.signed_duration_since(self.epoch()).num_milliseconds() as f64 / 60e3,
        );
        let prediction = self.constants.propagate(minutes_since_epoch)?;
        Ok(prediction.into())
    }

    /// Calculate observation at time t.
    ///
    /// Returns a predicted local observation.
    pub fn observe_at<O: Observer>(&self, t: DateTime<Utc>, observer: &O) -> Result<Observation> {
        let observation = self
            .propagate(t)?
            .to_ecef(t)
            .to_enu(observer)
            .to_observation();
        Ok(observation)
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

    /// Detect apogee and perigee events over a time interval.
    ///
    /// Returns an iterator over apsis events in the TEME frame.
    pub fn apsis_iter(&self, interval: impl IntervalRange) -> ApsisIter {
        ApsisIter::new(self.clone(), interval)
    }

    /// Calculate all of the transits visible to the observer.
    ///
    /// Returns an iterator over transits.
    pub fn transits_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation: f64,
    ) -> TransitIter<'a, O> {
        TransitIter::new(self.clone(), observer, interval, min_elevation)
    }

    /// Find the peak elevation of the satellite over an observer within a time interval.
    ///
    /// Scans in 10-second steps to bracket the point where the elevation rate crosses
    /// zero (ascending → descending), then refines with Brent's method to 1 ms accuracy.
    /// If no sign change is found (satellite never peaks within the interval), a
    /// roots::Error::Unbracketed is returned.
    pub fn max_elevation<O: Observer>(
        &self,
        interval: impl IntervalRange,
        observer: &O,
    ) -> Result<(DateTime<Utc>, Observation)> {
        const SCAN_STEP: Duration = Duration::seconds(10);
        let start_t = interval.start();
        let end_t = interval.end();

        let mut prev: Option<(f64, f64)> = None; // (t_f64, el_rate)
        let mut t = start_t;

        while t <= end_t {
            let t_f64 = time::datetime_to_f64(t);
            let (_, el_rate) = self
                .propagate(t)?
                .to_ecef(t)
                .to_enu(observer)
                .elevation_and_rate();

            if let Some((prev_t, prev_er)) = prev
                && prev_er > 0.0
                && el_rate < 0.0
            {
                // el_rate crossed zero: peak is bracketed in [prev_t, t_f64]
                let peak_t_f64 = roots::brent(
                    prev_t,
                    t_f64,
                    |x| {
                        let tx = time::f64_to_datetime(x);
                        self.propagate(tx)
                            .map(|s| s.to_ecef(tx).to_enu(observer).elevation_and_rate().1)
                    },
                    1e-3,
                    50,
                )
                .map_err(Error::Roots)?;

                let peak_t = time::f64_to_datetime(peak_t_f64);
                return Ok((peak_t, self.observe_at(peak_t, observer)?));
            }

            prev = Some((t_f64, el_rate));
            t += SCAN_STEP;
        }

        // No sign change found — no peak within the interval
        Err(Error::Roots(roots::Error::Unbracketed))
    }

    /// Determine whether the satellite is in sunlight or eclipse at time t.
    ///
    /// Uses a cylindrical Earth shadow model: the satellite is in eclipse when it
    /// is on the anti-Sun side of Earth and within one Earth radius of the
    /// Earth–Sun axis.
    pub fn is_sunlit(&self, t: DateTime<Utc>) -> Result<bool> {
        Ok(illumination::shadow_value(self, t)? < 0.0)
    }

    /// Detect all sunlit and eclipse windows over a time interval.
    ///
    /// Returns an iterator over illumination windows, each clamped to the search
    /// interval. Uses a cylindrical Earth shadow model with 60-second scan steps
    /// and Brent's method to refine shadow-boundary crossings to millisecond accuracy.
    pub fn illumination_iter(&self, interval: impl IntervalRange) -> IlluminationIter {
        IlluminationIter::new(self.clone(), interval)
    }

    /// Return the epoch of the TLE.
    pub fn epoch(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(self.elements.datetime, Utc)
    }

    /// Return the age of the TLE relative to `now`.
    ///
    /// Positive means the TLE epoch is in the past (normal operation).
    pub fn tle_age(&self, now: DateTime<Utc>) -> Duration {
        now.signed_duration_since(self.epoch())
    }
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("TLE parse error: {0}")]
    Tle(#[from] sgp4::TleError),
    #[error("SGP4 elements error: {0}")]
    Elements(#[from] sgp4::ElementsError),
    #[error("SGP4 propagation error: {0}")]
    Sgp4(#[from] sgp4::Error),
    #[error("Interval error: {0}")]
    Interval(String),
    #[error("Roots error: {0}")]
    Roots(#[from] roots::Error),
    #[error("Transit error: {0}")]
    Transit(#[from] transits::Error),
}
