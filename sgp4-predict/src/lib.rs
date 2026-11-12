//! Higher-level satellite prediction built on the [`sgp4`] crate.
//!
//! [`Predictor`] is the main entry point. Construct it from a [`Tle`] (or any
//! type convertible to [`Elements`]), then use its methods to propagate state
//! vectors, compute ground observations, detect passes, find apsides, and query
//! illumination.
//!
//! # Quick start
//!
//! ```no_run
//! use sgp4_predict::{Degrees, GeodeticPoint, Predictor, Tle};
//! use chrono::{Duration, Utc};
//!
//! let tle: Tle = "\
//!     SENTINEL-2C\n\
//!     1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990\n\
//!     2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
//!     .parse()
//!     .unwrap();
//!
//! let predictor = Predictor::new(&tle).unwrap();
//! let glasgow = GeodeticPoint {
//!     latitude: Degrees(55.86),
//!     longitude: Degrees(-4.25),
//!     altitude: 40.0,
//! };
//!
//! let start = Utc::now();
//! let end = start + Duration::days(1);
//!
//! for transit in predictor.transits_iter(glasgow, start..end, Degrees(5.0)) {
//!     let transit = transit.unwrap();
//!     println!("AoS: {}  LoS: {}", transit.start, transit.end);
//! }
//! ```
//!
//! # Custom types
//!
//! If your application already has types that hold TLE data or coordinates,
//! implement `TryFrom<&YourType> for `[`Elements`] and
//! `From<&YourType> for `[`GeodeticPoint`], and pass them to [`Predictor::new`]
//! directly.
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
//! [`GeodeticPoint::latitude`]/[`longitude`](GeodeticPoint::longitude) are
//! [`Degrees`], [`Observation::azimuth`]/[`elevation`](Observation::elevation)
//! are [`Radians`], and `min_elevation` parameters accept either.
//!
//! # Handling errors
//!
//! Every iterator yields [`Result`]. [`FallibleIter`] turns the loop-body `?`
//! into one call on the iterator: `.log_errors()` to skip a failed event,
//! `.tolerate_errors(n)` to stop once failures persist.
//!
//! # Generic event detection
//!
//! Every detection iterator finds its events the same
//! way: a scalar event function `f(t)` is sampled across the interval by a
//! step strategy, a sign change between two samples brackets a crossing, and
//! the crossing time is refined by a bracketed hybrid solver. The concrete
//! iterators are thin wrappers over that layer.
//!
//! The off-by-default `generics` Cargo feature exposes it, so an event kind
//! this crate does not cover needs no bespoke iterator:
//!
//! - `DetectIter` is the driving loop: it repeatedly asks a `Detector` for the
//!   next sample time and whether an event completed there. Implement
//!   `Detector` for fully custom detection.
//! - `CrossingDetector` (built via `EventIter::builder`) finds the zero
//!   crossings of a scalar `EventFunction` — point-in-time **events**, such as
//!   apsides.
//! - `WindowDetector` (built via `WindowIter::builder`) pairs crossings into
//!   **windows** — intervals over which `f(t)` keeps one sign, such as transits
//!   or illumination.
//! - `StepStrategy` decides how far to advance between samples: `FixedStep`
//!   scans uniformly, `ThresholdStep` takes large steps far from a threshold
//!   crossing and small ones near it.
//!
//! Each refinement iteration takes a Newton-Raphson step when its sample
//! carries a derivative (`Sample::rate`) and a secant/bisection step otherwise,
//! converging once the crossing is pinned down to the solver's time tolerance.
//!
//! In the TEME frame the equator is the plane `z = 0`, so ascending-node
//! crossings are the rising zero crossings of the satellite's z coordinate:
//!
#![cfg_attr(feature = "generics", doc = "```no_run")]
#![cfg_attr(not(feature = "generics"), doc = "```ignore")]
//! use chrono::{Duration, Utc};
//! use sgp4_predict::{Direction, EventIter, FixedStep, Predictor, Tle};
//!
//! # let tle: Tle = "ISS (ZARYA)\n1 ...\n2 ...".parse().unwrap();
//! let predictor = Predictor::new(&tle).unwrap();
//! let start = Utc::now();
//!
//! let crossings = EventIter::builder()
//!     .interval(start..start + Duration::days(1))
//!     .function(move |t| Ok(Some(predictor.propagate(t)?.position.z)))
//!     .step(FixedStep(Duration::seconds(60)))
//!     .build()
//!     .unwrap();
//!
//! for crossing in crossings {
//!     let crossing = crossing.unwrap();
//!     if crossing.direction == Direction::Rising {
//!         println!("northward equator crossing at {}", crossing.time);
//!     }
//! }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

/// Compiles the README's examples as doctests so they cannot drift.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;

mod angle;
mod aoi;
mod apsides;
mod arg_lat;
mod detect;
mod fallible;
mod frames;
mod illumination;
mod observe;
mod orbital_elements;
mod predict;
mod roots;
mod time;
mod transits;
mod types;
mod vectors;

use chrono::{DateTime, Duration, Utc};
use sgp4::{Constants, MinutesSinceEpoch};
use std::str::FromStr;
use thiserror::Error as ThisError;

pub use sgp4::{Classification, Elements, TleError};

pub use crate::{
    angle::{Degrees, Radians},
    aoi::{
        AoiIter, AoiIterOpts, AoiWindow, Area, Circle, Coverage, Error as AoiError, FillRule,
        Polygon, Rectangle,
    },
    apsides::{Apsis, ApsisEvent, ApsisIter, ApsisIterOpts},
    arg_lat::{ArgLatIter, ArgLatIterOpts},
    detect::Error as DetectError,
    fallible::{FallibleIter, OnError, Tolerate},
    frames::{EcefState, EnuState, GeodeticPoint, LatLon, LvlhDirection, LvlhState, TemeState},
    illumination::{Illumination, IlluminationIter, IlluminationIterOpts, IlluminationState},
    observe::{Observation, ObservationIter, Pointing},
    orbital_elements::{KeplerianElements, MeanElements},
    predict::{GroundTrackIter, PredictionIter},
    roots::{Error as RootsError, Refinement},
    time::{DateTimeIter, IntervalRange, TimeWindow},
    transits::{MaxElevationOpts, Transit, TransitIter, TransitIterOpts},
    types::{Tle, TleParseError},
    vectors::{Position, StateVector, UnitVector, Velocity},
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
        GeodeticPoint, IlluminationState, LatLon, Observation, Pointing, Polygon, Predictor,
        Radians, Result, TimeWindow, Tle, Transit,
    };
}

/// Crate-wide result type, parameterised over [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Pre-computed SGP4 constants ready for propagation.
///
/// Build from a [`Tle`], or from pre-parsed [`sgp4::Elements`] (e.g. from an
/// OMM JSON object), via [`Predictor::new`].
#[derive(Debug, Clone)]
pub struct Predictor {
    elements: Elements,
    constants: Constants,
    refinement: Refinement,
}

/// Parse a 2- or 3-line element set directly.
///
/// ```
/// # use sgp4_predict::Predictor;
/// let predictor: Predictor = "\
///     SENTINEL-2C\n\
///     1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990\n\
///     2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
///     .parse()
///     .unwrap();
/// ```
impl FromStr for Predictor {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let (name, line_1, line_2) = types::split_element_set(s)?;
        Self::new(Elements::from_tle(
            Some(name.into_owned()),
            line_1.as_bytes(),
            line_2.as_bytes(),
        )?)
    }
}

impl Predictor {
    /// Initialise SGP4 constants from orbital elements.
    ///
    /// Accepts [`Elements`] (e.g. deserialised from an OMM JSON object with
    /// `serde_json`), a [`Tle`] or `&`[`Tle`], or any type with a
    /// `TryFrom<&YourType> for Elements` impl — see [`Elements::from_tle`] for
    /// building one from TLE lines.
    ///
    /// Returns an error if conversion or SGP4 element initialisation fails.
    ///
    /// SGP4 accuracy degrades with element-set age (typically beyond 3–7 days for LEO).
    /// Use [`tle_age`](Predictor::tle_age) to check staleness and warn or reject
    /// as appropriate for your use case.
    pub fn new<E>(elements: E) -> Result<Self>
    where
        E: TryInto<Elements>,
        E::Error: Into<Error>,
    {
        let elements = elements.try_into().map_err(Into::into)?;
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
            "predictor initialized"
        );
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
    pub fn sub_point(&self, t: DateTime<Utc>) -> Result<GeodeticPoint> {
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
    /// The TLE text was not two or three usable lines.
    #[error("TLE format error: {0}")]
    TleFormat(#[from] TleParseError),
    /// A TLE line was malformed.
    #[error("TLE parse error: {0}")]
    Tle(#[from] sgp4::TleError),
    /// SGP4 rejected the orbital elements.
    #[error("SGP4 elements error: {0}")]
    Elements(#[from] sgp4::ElementsError),
    /// Propagation failed — typically a decayed or otherwise degenerate orbit,
    /// which fails at every time, not just the one sampled.
    #[error("SGP4 propagation error: {0}")]
    Sgp4(#[from] sgp4::Error),
    /// A time interval was unusable.
    #[error("Interval error: {0}")]
    Interval(String),
    /// The root finder failed to pin down an event time.
    #[error("Roots error: {0}")]
    Roots(#[from] roots::Error),
    /// Event or window detection failed.
    #[error("Detection error: {0}")]
    Detect(#[from] detect::Error),
    /// An [`Area`] could not be constructed from the given geometry.
    #[error("Area of interest error: {0}")]
    Aoi(#[from] aoi::Error),
    /// An argument of latitude was NaN or infinite.
    #[error("argument of latitude is not finite: {0}")]
    NonFiniteArgumentOfLatitude(f64),
    /// The orbit is equatorial, so the argument of latitude is undefined: it
    /// is measured from the ascending node, and there is none.
    #[error("equatorial orbit has no ascending node")]
    NoAscendingNode,
    /// The state is on a parabolic or hyperbolic orbit, so it has no
    /// semi-major axis or period.
    #[error("orbit is not bound")]
    NotBound,
    /// Position and velocity are parallel, so there is no angular momentum
    /// and no orbit plane.
    #[error("state has no orbit plane")]
    NoOrbitPlane,
    /// Escape hatch for custom `EventFunction` implementations (`generics`
    /// feature) whose failures don't fit another variant.
    #[error("{0}")]
    Custom(String),
}

// `Elements: TryInto<Elements>` fails with `Infallible`, which `Predictor::new`'s
// `E::Error: Into<Error>` bound needs to convert. It has no values, so this never runs.
impl From<std::convert::Infallible> for Error {
    fn from(never: std::convert::Infallible) -> Self {
        match never {}
    }
}
