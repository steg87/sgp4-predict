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
//! use sgp4_predict::{Degrees, GroundObserver, Predictor, Tle};
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
//! let glasgow = GroundObserver::new(Degrees(55.86), Degrees(-4.25), 40.0);
//!
//! let start = Utc::now();
//! let end = start + Duration::days(1);
//!
//! for transit in predictor.transits_iter(&glasgow, start..end, Degrees(5.0)) {
//!     let transit = transit.unwrap();
//!     println!("AoS: {}  LoS: {}", transit.start, transit.end);
//! }
//! ```
//!
//! # Custom types
//!
//! If your application already has types that hold TLE data or coordinates,
//! implement [`TleRecord`] and [`Observer`] on them instead of converting to
//! [`Tle`] / [`GroundObserver`].
//!
//! # OMM support
//!
//! [`sgp4::Elements`] (re-exported as [`Elements`]) holds orbital data from
//! either a TLE or an OMM (Orbit Mean-Elements Message). It derives
//! `serde::Deserialize` with the CCSDS field names (`NORAD_CAT_ID`, `EPOCH`,
//! `MEAN_MOTION`, ...), so Celestrak and Space-Track JSON parses directly and
//! can be handed to [`Predictor::new`]:
//!
//! ```no_run
//! use sgp4_predict::{Elements, Predictor};
//!
//! # let omm_json = "{}";
//! let elements: Elements = serde_json::from_str(omm_json).unwrap();
//! let predictor = Predictor::new(elements).unwrap();
//! ```
//!
//! # Units
//!
//! Positions are in **metres** and velocities in **m/s**. Angles are typed:
//! [`Observer::latitude`]/[`longitude`](Observer::longitude) take [`Degrees`],
//! [`Observation::azimuth`]/[`elevation`](Observation::elevation) are
//! [`Radians`], and `min_elevation` parameters accept either.
//!
//! # Handling errors
//!
//! Every iterator yields [`Result`]. [`FallibleIter`] turns the loop-body `?`
//! into one call on the iterator: `.log_errors()` to skip a failed event,
//! `.tolerate_errors(n)` to stop once failures persist.
//!
//! # Cargo features
//!
//! - `generics` — exposes the generic event/window detection building blocks
//!   (`EventIter`, `WindowIter`, `Detector`, `StepStrategy`, ...) that power
//!   the concrete iterators, for detecting event kinds this crate does not
//!   cover. Off by default.

#![cfg_attr(docsrs, feature(doc_cfg))]

/// Compiles the README's examples as doctests so they cannot drift.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;

mod angle;
mod aoi;
mod apsides;
mod detect;
mod fallible;
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
    angle::{Degrees, Radians},
    aoi::{
        AoiIter, AoiIterOpts, AoiWindow, Area, Ellipse, Error as AoiError, FillRule, Polygon,
        Rectangle,
    },
    apsides::{Apsis, ApsisEvent, ApsisIter, ApsisIterOpts},
    detect::Error as DetectError,
    fallible::{FallibleIter, OnError, Tolerate},
    frames::{EcefState, EnuState, Geodetic, LatLon, TemeState},
    illumination::{Illumination, IlluminationIter, IlluminationIterOpts, IlluminationState},
    observe::{Observation, ObservationIter, Observer},
    predict::{GroundTrackIter, PredictionIter},
    roots::Refinement,
    time::{DateTimeIter, IntervalRange, TimeWindow},
    transits::{MaxElevationOpts, Transit, TransitIter, TransitIterOpts},
    types::{GroundObserver, Tle, TleParseError},
    vectors::{Position, StateVector, Velocity},
};

#[cfg(feature = "generics")]
pub use crate::detect::{
    Crossing, CrossingDetector, DetectIter, Detector, Direction, EventFunction, EventIter,
    EventIterBuilder, FixedStep, Missing, RateFn, Sample, StepStrategy, ThresholdStep, ValueFn,
    Window, WindowDetector, WindowIter, WindowIterBuilder,
};

/// Commonly used types for quick onboarding.
///
/// ```
/// use sgp4_predict::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        AoiWindow, ApsisEvent, Classification, Degrees, Elements, Error, FallibleIter,
        GroundObserver, IlluminationState, LatLon, Observation, Observer, Polygon, Predictor,
        Radians, Result, TimeWindow, Tle, TleRecord, Transit,
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
    #[must_use = "returns a reconfigured Predictor; the receiver is unchanged"]
    pub fn with_refinement(mut self, refinement: Refinement) -> Self {
        self.refinement = refinement;
        self
    }

    /// The root-finder configuration currently in effect.
    ///
    /// Useful when calling a `*_iter_with_opts` method: `refinement` there is
    /// a separate argument from `opts`, so pass `predictor.refinement()` to
    /// preserve whatever this `Predictor` was already configured with instead
    /// of silently reverting to [`Refinement::default()`].
    #[must_use]
    pub fn refinement(&self) -> Refinement {
        self.refinement
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

    /// The sub-satellite point at `t`: the geodetic position directly beneath
    /// the satellite, with `altitude` its height above the WGS-84 ellipsoid.
    ///
    /// Sampling this over an interval traces the ground track.
    pub fn sub_point(&self, t: DateTime<Utc>) -> Result<Geodetic> {
        Ok(self.propagate(t)?.to_ecef(t).to_geodetic())
    }

    /// Return the epoch of the TLE.
    #[must_use]
    pub fn epoch(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(self.elements.datetime, Utc)
    }

    /// Return the age of the TLE relative to `now`.
    ///
    /// Positive means the TLE epoch is in the past (normal operation).
    #[must_use]
    pub fn tle_age(&self, now: DateTime<Utc>) -> Duration {
        now.signed_duration_since(self.epoch())
    }
}

/// Errors returned by this crate.
#[derive(Debug, Clone, PartialEq, ThisError)]
#[non_exhaustive]
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
    #[error("Detection error: {0}")]
    Detect(#[from] detect::Error),
    #[error("Area of interest error: {0}")]
    Aoi(#[from] aoi::Error),
    /// Escape hatch for custom `EventFunction` implementations (`generics`
    /// feature) whose failures don't fit another variant.
    #[error("{0}")]
    Custom(String),
}
