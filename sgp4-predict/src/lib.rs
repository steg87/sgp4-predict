//! Higher-level satellite prediction built on the [`sgp4`] crate.
//!
//! [`Predictor`] is the main entry point. Construct it from any type that
//! implements [`Satellite`] (i.e. [`HasId`] + [`HasTle`]), then use its
//! methods to propagate state vectors, compute ground observations, detect
//! passes, find apsides, and query illumination.
//!
//! # Units
//!
//! All positions are in **metres** and velocities in **m/s**.
//! Observer latitude and longitude must be supplied in **degrees**.

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
    frames::{EcefState, EnuState, TemeState},
    illumination::{Illumination, IlluminationIter, IlluminationState},
    observe::{Observation, ObservationIter, Observer},
    predict::PredictionIter,
    roots::{Brent, NewtonRaphson, Refinement},
    time::{DateTimeIter, IntervalRange},
    transits::{Transit, TransitIter},
    vectors::{Position, StateVector, Velocity},
};

/// Crate-wide result type, parameterised over [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Marker supertrait for a satellite that has both an identifier and TLE lines.
///
/// Any type implementing both [`HasId`] and [`HasTle`] automatically
/// implements `Satellite`.
pub trait Satellite: HasId + HasTle {}
impl<T> Satellite for T where T: HasId + HasTle {}

/// A type that provides a satellite identifier.
pub trait HasId {
    /// Returns the satellite name or NORAD catalog number string.
    fn id(&self) -> &str;
}

/// A type that provides the two lines of a TLE.
pub trait HasTle {
    /// Returns TLE line 1.
    fn line_1(&self) -> &str;
    /// Returns TLE line 2.
    fn line_2(&self) -> &str;
}

/// Parsed TLE with pre-computed SGP4 constants, ready for propagation.
///
/// Construct with [`Predictor::new`] from any [`Satellite`].
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
    refinement: Refinement,
}

impl Predictor {
    /// Parse a TLE and initialise SGP4 constants.
    ///
    /// Returns an error if the TLE text is malformed or if SGP4
    /// element initialisation fails.
    ///
    /// SGP4 accuracy degrades with TLE age (typically beyond 3–7 days for LEO).
    /// Use [`tle_age`](Predictor::tle_age) to check staleness and warn or reject
    /// as appropriate for your use case.
    pub fn new(sat: &impl Satellite) -> Result<Self> {
        let elements = Elements::from_tle(
            Some(sat.id().to_owned()),
            sat.line_1().as_bytes(),
            sat.line_2().as_bytes(),
        )?;
        let constants = Constants::from_elements(&elements)?;
        let predictor = Self {
            elements,
            constants,
            refinement: Refinement::default(),
        };
        tracing::debug!(satellite = sat.id(), epoch = %predictor.epoch(), "predictor initialized");
        Ok(predictor)
    }

    /// Set the root-finder configuration used by [`detect_transit`] and [`max_elevation`].
    ///
    /// [`detect_transit`]: Predictor::detect_transit
    /// [`max_elevation`]: Predictor::max_elevation
    pub fn with_refinement(mut self, refinement: Refinement) -> Self {
        self.refinement = refinement;
        self
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
        ApsisIter::new(self.clone(), interval).with_brent(self.refinement.brent)
    }

    /// Calculate all of the transits visible to the observer.
    ///
    /// `min_elevation_deg` is the minimum elevation above the horizon in **degrees**.
    ///
    /// Returns an iterator over transits.
    pub fn transits_iter<'a, O: Observer>(
        &self,
        observer: &'a O,
        interval: impl IntervalRange,
        min_elevation_deg: f64,
    ) -> TransitIter<'a, O> {
        TransitIter::new(
            self.clone(),
            observer,
            interval,
            min_elevation_deg.to_radians(),
        )
        .with_refinement(self.refinement)
    }

    /// Detect whether a transit is in progress at time `t`.
    ///
    /// `min_elevation_deg` is the minimum elevation above the horizon in **degrees**.
    ///
    /// If the satellite is below `min_elevation_deg` at `t`, returns `Ok(None)`.
    /// Otherwise, searches backward and forward in 30-second steps to bracket the
    /// AoS and LoS crossings, then refines each boundary with Newton-Raphson /
    /// Brent's method to millisecond accuracy.
    ///
    /// Returns an error if either boundary is not found within 1 hour.
    pub fn detect_transit<O: Observer>(
        &self,
        t: DateTime<Utc>,
        observer: &O,
        min_elevation_deg: f64,
    ) -> Result<Option<Transit>> {
        let min_elevation = min_elevation_deg.to_radians();
        let calculate = |t: DateTime<Utc>| -> Result<(f64, f64)> {
            let (el, el_rate) = self
                .propagate(t)?
                .to_ecef(t)
                .to_enu(observer)
                .elevation_and_rate();
            Ok((el, el_rate))
        };

        let mut f = |t: f64| {
            calculate(time::f64_to_datetime(t)).map(|(el, el_rate)| (el - min_elevation, el_rate))
        };

        let (el, _) = calculate(t)?;
        if el < min_elevation {
            return Ok(None);
        }

        const STEP: Duration = Duration::seconds(30);

        // --- Find start (search backward) ---
        let mut t_inner = t;
        let mut t_outer = t - STEP;
        let start = loop {
            if t - t_outer > Duration::hours(1) {
                tracing::warn!(at = %t, "transit start not found within 1 hour");
                return Err(transits::Error::TransitStartNotFound { at: t }.into());
            }
            let (el, _) = calculate(t_outer)?;
            if el < min_elevation {
                let s = self.refinement.hybrid_solve(
                    time::datetime_to_f64(t_outer),
                    time::datetime_to_f64(t_inner),
                    &mut f,
                )?;
                break time::f64_to_datetime(s);
            }
            t_inner = t_outer;
            t_outer -= STEP;
        };

        // --- Find end (search forward) ---
        let mut t_inner = t;
        let mut t_outer = t + STEP;
        let end = loop {
            if t_outer - t > Duration::hours(1) {
                tracing::warn!(%start, "transit end not found within 1 hour");
                return Err(transits::Error::TransitEndNotFound { start }.into());
            }
            let (el, _) = calculate(t_outer)?;
            if el < min_elevation {
                let e = self.refinement.hybrid_solve(
                    time::datetime_to_f64(t_inner),
                    time::datetime_to_f64(t_outer),
                    &mut f,
                )?;
                break time::f64_to_datetime(e);
            }
            t_inner = t_outer;
            t_outer += STEP;
        };

        let transit = Transit::new(start, end);
        tracing::debug!(aos = %transit.start, los = %transit.end, "transit detected");
        Ok(Some(transit))
    }

    /// Find the peak elevation of the satellite over an observer within a time interval.
    ///
    /// Scans in 10-second steps to bracket the point where the elevation rate crosses
    /// zero (ascending → descending), then refines with Brent's method.
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
                let peak_t_f64 = self
                    .refinement
                    .brent
                    .solve(prev_t, t_f64, |x| {
                        let tx = time::f64_to_datetime(x);
                        self.propagate(tx)
                            .map(|s| s.to_ecef(tx).to_enu(observer).elevation_and_rate().1)
                    })
                    .map_err(Error::Roots)?;

                let peak_t = time::f64_to_datetime(peak_t_f64);
                let obs = self.observe_at(peak_t, observer)?;
                tracing::debug!(time = %peak_t, elevation_deg = obs.elevation.to_degrees(), "peak elevation found");
                return Ok((peak_t, obs));
            }

            prev = Some((t_f64, el_rate));
            t += SCAN_STEP;
        }

        // No sign change found — no peak within the interval
        Err(Error::Roots(roots::Error::Unbracketed))
    }

    /// Determine whether the satellite is sunlit or in eclipse at time t.
    ///
    /// Uses a cylindrical Earth shadow model: the satellite is in eclipse when it
    /// is on the anti-Sun side of Earth and within one Earth radius of the
    /// Earth–Sun axis.
    pub fn illumination_state(&self, t: DateTime<Utc>) -> Result<IlluminationState> {
        Ok(if illumination::shadow_value(self, t)? < 0.0 {
            IlluminationState::Sunlit
        } else {
            IlluminationState::Eclipse
        })
    }

    /// Detect all sunlit and eclipse windows over a time interval.
    ///
    /// Returns an iterator over illumination windows, each clamped to the search
    /// interval. Uses a cylindrical Earth shadow model with 60-second scan steps
    /// and Brent's method to refine shadow-boundary crossings to millisecond accuracy.
    pub fn illumination_iter(&self, interval: impl IntervalRange) -> IlluminationIter {
        IlluminationIter::new(self.clone(), interval).with_brent(self.refinement.brent)
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

/// Errors returned by this crate.
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
