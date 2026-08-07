//! Standard implementations of [`TleRecord`] and [`Observer`].
//!
//! These concrete types let you get started quickly without defining your own
//! structs. They are also available via [`crate::prelude`].

use sgp4::{Elements, TleError};
use std::str::FromStr;

use thiserror::Error as ThisError;

use crate::{TleRecord, angle::Degrees, observe::Observer};

/// Error returned when a [`Tle`] cannot be parsed from a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, ThisError)]
#[error("invalid TLE: {0}")]
pub struct TleParseError(pub(crate) String);

/// A TLE (Two-Line Element set) with an associated satellite identifier.
///
/// Implements [`TleRecord`] so it can be passed directly to
/// [`Predictor::from_tle`][crate::Predictor::from_tle].
///
/// # Construction
///
/// Build directly with [`Tle::new`], or parse a standard 3-line element set
/// (name, line 1, line 2) with [`FromStr`]:
///
/// ```
/// use sgp4_predict::Tle;
///
/// let tle: Tle = "\
///     SENTINEL-2C\n\
///     1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990\n\
///     2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
///     .parse()
///     .unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tle {
    /// Satellite name or NORAD catalog number string.
    pub satellite_name: String,
    /// TLE line 1.
    pub line_1: String,
    /// TLE line 2.
    pub line_2: String,
}

impl Tle {
    /// Construct a [`Tle`] from its three components.
    pub fn new(
        satellite_name: impl Into<String>,
        line_1: impl Into<String>,
        line_2: impl Into<String>,
    ) -> Self {
        Self {
            satellite_name: satellite_name.into(),
            line_1: line_1.into(),
            line_2: line_2.into(),
        }
    }
}

impl TleRecord for Tle {
    fn satellite_name(&self) -> &str {
        &self.satellite_name
    }

    fn line_1(&self) -> &str {
        &self.line_1
    }

    fn line_2(&self) -> &str {
        &self.line_2
    }
}

impl TryFrom<Tle> for Elements {
    type Error = TleError;

    fn try_from(tle: Tle) -> Result<Self, TleError> {
        Elements::from_tle(
            Some(tle.satellite_name),
            tle.line_1.as_bytes(),
            tle.line_2.as_bytes(),
        )
    }
}

impl TryFrom<&Tle> for Elements {
    type Error = TleError;

    fn try_from(tle: &Tle) -> Result<Self, TleError> {
        Elements::from_tle(
            Some(tle.satellite_name.clone()),
            tle.line_1.as_bytes(),
            tle.line_2.as_bytes(),
        )
    }
}

/// Parse a standard 3-line element set (name + two TLE lines).
///
/// Blank lines are ignored; exactly three non-empty lines must remain.
///
/// # Errors
///
/// Returns [`TleParseError`] if the input does not contain exactly three
/// non-empty lines.
impl FromStr for Tle {
    type Err = TleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        match lines.as_slice() {
            [satellite_name, line_1, line_2] => Ok(Self::new(*satellite_name, *line_1, *line_2)),
            _ => Err(TleParseError(format!(
                "expected 3 non-empty lines (name, line 1, line 2), got {}",
                lines.len()
            ))),
        }
    }
}

/// A fixed point on Earth's surface from which satellite passes are observed.
///
/// Implements [`Observer`] so it can be passed to
/// [`Predictor::observe_at`][crate::Predictor::observe_at],
/// [`Predictor::observation_iter`][crate::Predictor::observation_iter], and
/// [`Predictor::transits_iter`][crate::Predictor::transits_iter].
///
/// Altitude is in **metres** above the WGS-84 ellipsoid.
///
/// # Example
///
/// ```
/// use sgp4_predict::{Degrees, GroundObserver};
///
/// // London, ~20 m ASL
/// let london = GroundObserver::new(Degrees(51.5074), Degrees(-0.1278), 20.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundObserver {
    /// Geodetic latitude (positive north).
    latitude: Degrees,
    /// Geodetic longitude (positive east).
    longitude: Degrees,
    /// Height above the WGS-84 ellipsoid in metres.
    altitude: f64,
}

impl GroundObserver {
    /// Construct a [`GroundObserver`].
    ///
    /// - `latitude` — geodetic latitude (positive north)
    /// - `longitude` — geodetic longitude (positive east)
    /// - `altitude` — height above the WGS-84 ellipsoid in metres
    pub const fn new(latitude: Degrees, longitude: Degrees, altitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude,
        }
    }
}

impl Observer for GroundObserver {
    fn latitude(&self) -> Degrees {
        self.latitude
    }

    fn longitude(&self) -> Degrees {
        self.longitude
    }

    fn altitude(&self) -> f64 {
        self.altitude
    }
}
