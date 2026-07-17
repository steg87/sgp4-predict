//! Higher-level satellite prediction built on the [`sgp4`] crate.
//!
//! [`Predictor`] is the main entry point. Construct it from a [`Tle`] (or any
//! type that implements [`TleRecord`]), then use its methods to propagate state
//! vectors, compute ground observations, detect passes, find apsides, and query
//! illumination.
//!
//! # Quick start
//!
//! ```no_run
//! use sgp4_predict::{GroundObserver, Predictor, Tle};
//! use chrono::{Duration, Utc};
//!
//! let tle: Tle = "\
//!     SENTINEL-2C\n\
//!     1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990\n\
//!     2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
//!     .parse()
//!     .unwrap();
//!
//! let predictor = Predictor::from_tle(&tle).unwrap();
//! let glasgow = GroundObserver::new(55.86, -4.25, 40.0);
//!
//! let start = Utc::now();
//! let end = start + Duration::days(1);
//!
//! for transit in predictor.transits_iter(&glasgow, start..end, 5.0) {
//!     let transit = transit.unwrap();
//!     println!("AoS: {}  LoS: {}", transit.start, transit.end);
//! }
//! ```
//!
//! # Custom types
//!
//! If your application already has types that hold TLE data or coordinates,
//! implement [`TleRecord`] and [`Observer`] instead of converting to [`Tle`] /
//! [`GroundObserver`]. Pass your type to [`Predictor::from_tle`]. See the
//! trait docs for details.
//!
//! # OMM support
//!
//! [`sgp4::Elements`] (re-exported as [`Elements`]) represents orbital data
//! from either a TLE or an OMM (Orbit Mean-Elements Message). Because it
//! derives `serde::Deserialize`, you can parse an OMM JSON object directly
//! with `serde_json` and hand the result to [`Predictor::new`]:
//!
//! ```no_run
//! use sgp4_predict::{Elements, Predictor};
//!
//! # let omm_json = "{}";
//! let elements: Elements = serde_json::from_str(omm_json).unwrap();
//! let predictor = Predictor::new(elements).unwrap();
//! ```
//!
//! The JSON field names follow the CCSDS OMM standard
//! (`NORAD_CAT_ID`, `EPOCH`, `MEAN_MOTION`, `ECCENTRICITY`, etc.).
//! Both Celestrak and Space-Track JSON responses parse directly into
//! `Elements` with no extra mapping required.
//!
//! # Units
//!
//! All positions are in **metres** and velocities in **m/s**.
//! Observer latitude and longitude must be supplied in **degrees**.

mod apsides;
mod detect;
mod frames;
mod illumination;
mod observe;
mod predict;
mod roots;
mod time;
mod transits;
mod types;
mod vectors;

use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, MinutesSinceEpoch};
use thiserror::Error as ThisError;

pub use sgp4::{Classification, Elements};

pub use crate::{
    apsides::{Apsis, ApsisEvent, ApsisIter},
    detect::{
        Crossing, CrossingDetector, DetectIter, Detector, Direction, Error as DetectError,
        EventFunction, EventIter, EventIterBuilder, FixedStep, Missing, RateFn, Sample,
        StepStrategy, ThresholdStep, ValueFn, Window, WindowDetector, WindowIter,
        WindowIterBuilder,
    },
    frames::{EcefState, EnuState, TemeState},
    illumination::{Illumination, IlluminationIter, IlluminationState},
    observe::{Observation, ObservationIter, Observer},
    predict::PredictionIter,
    roots::Refinement,
    time::{DateTimeIter, IntervalRange},
    transits::{Transit, TransitIter},
    types::{GroundObserver, Tle, TleParseError},
    vectors::{Position, StateVector, Velocity},
};

/// Commonly used types for quick onboarding.
///
/// ```
/// use sgp4_predict::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        ApsisEvent, Classification, Elements, Error, GroundObserver, IlluminationState,
        Observation, Observer, Predictor, Result, Tle, TleRecord, Transit,
    };
}

/// Crate-wide result type, parameterised over [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// A type that holds a satellite name and two TLE lines.
///
/// Implement this on your own struct to pass it directly to
/// [`Predictor::from_tle`] without converting to [`Tle`] first.
pub trait TleRecord {
    /// Returns the satellite name or NORAD catalog number string.
    fn satellite_name(&self) -> &str;
    /// Returns TLE line 1.
    fn line_1(&self) -> &str;
    /// Returns TLE line 2.
    fn line_2(&self) -> &str;
}

impl<T: TleRecord> TleRecord for &T {
    fn satellite_name(&self) -> &str {
        (*self).satellite_name()
    }
    fn line_1(&self) -> &str {
        (*self).line_1()
    }
    fn line_2(&self) -> &str {
        (*self).line_2()
    }
}

/// Pre-computed SGP4 constants ready for propagation.
///
/// Build from TLE string lines via [`Predictor::from_tle`], or from a
/// pre-parsed [`sgp4::Elements`] (e.g. from an OMM JSON object) via
/// [`Predictor::new`].
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
    refinement: Refinement,
}

impl Predictor {
    /// Initialise SGP4 constants from pre-parsed orbital elements.
    ///
    /// Use this when you already have a [`sgp4::Elements`] value — for
    /// example, one deserialised from an OMM JSON object with `serde_json`.
    /// For the common case of building from TLE string lines, prefer
    /// [`from_tle`](Predictor::from_tle).
    ///
    /// Returns an error if SGP4 element initialisation fails.
    ///
    /// SGP4 accuracy degrades with element-set age (typically beyond 3–7 days for LEO).
    /// Use [`tle_age`](Predictor::tle_age) to check staleness and warn or reject
    /// as appropriate for your use case.
    pub fn new(elements: Elements) -> Result<Self> {
        let constants = Constants::from_elements(&elements)?;
        let predictor = Self {
            elements,
            constants,
            refinement: Refinement::default(),
        };
        tracing::debug!(
            satellite = %predictor.elements.object_name.as_deref().unwrap_or(
                &format!("NORAD {}", predictor.elements.norad_id)),
            epoch = %predictor.epoch(),
            "predictor initialized from OMM elements"
        );
        Ok(predictor)
    }

    /// Parse TLE string lines and initialise SGP4 constants.
    ///
    /// Accepts any type implementing [`TleRecord`], including [`Tle`], `&`[`Tle`],
    /// or your own custom struct.
    ///
    /// Returns an error if the TLE text is malformed or if SGP4
    /// element initialisation fails.
    pub fn from_tle(tle: impl TleRecord) -> Result<Self> {
        let id = tle.satellite_name().to_string();
        let elements = Elements::from_tle(
            Some(id.clone()),
            tle.line_1().as_bytes(),
            tle.line_2().as_bytes(),
        )?;
        let constants = Constants::from_elements(&elements)?;
        let predictor = Self {
            elements,
            constants,
            refinement: Refinement::default(),
        };
        tracing::debug!(satellite = %id, epoch = %predictor.epoch(), "predictor initialized from TLE");
        Ok(predictor)
    }

    /// Set the root-finder configuration used to refine event times across
    /// all detection iterators, [`detect_transit`], and [`max_elevation`].
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
        ApsisIter::new(self.clone(), interval).with_refinement(self.refinement)
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
    /// AoS and LoS crossings, then refines each boundary with the bracketed
    /// hybrid solver ([`Refinement`]) to millisecond accuracy.
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
            calculate(time::f64_to_datetime(t))
                .map(|(el, el_rate)| (el - min_elevation, Some(el_rate)))
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
                let s = self.refinement.solve(
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
                let e = self.refinement.solve(
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
    /// zero (ascending → descending), then refines the crossing with the bracketed
    /// hybrid solver ([`Refinement`]).
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
                // el_rate crossed zero: peak is bracketed in [prev_t, t_f64].
                // The event function here is the elevation *rate*, whose own
                // derivative is not available — samples carry no rate.
                let peak_t_f64 = self
                    .refinement
                    .solve(prev_t, t_f64, |x| {
                        let tx = time::f64_to_datetime(x);
                        self.propagate(tx)
                            .map(|s| (s.to_ecef(tx).to_enu(observer).elevation_and_rate().1, None))
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
    /// interval. Uses a cylindrical Earth shadow model with 60-second scan steps,
    /// refining shadow-boundary crossings to millisecond accuracy.
    pub fn illumination_iter(&self, interval: impl IntervalRange) -> IlluminationIter {
        IlluminationIter::new(self.clone(), interval).with_refinement(self.refinement)
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
    #[error("TLE format error: {0}")]
    TleFormat(#[from] TleParseError),
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
    #[error("Detection error: {0}")]
    Detect(#[from] detect::Error),
}
