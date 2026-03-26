//! Standard implementations of [`Satellite`] and [`Observer`].
//!
//! These concrete types let you get started quickly without defining your own
//! structs. They are also available via [`crate::prelude`].

use std::str::FromStr;

use thiserror::Error as ThisError;

use crate::{HasId, HasTle, observe::Observer};

/// Error returned when a [`Tle`] cannot be parsed from a string.
#[derive(Debug, ThisError)]
#[error("invalid TLE: {0}")]
pub struct TleParseError(pub(crate) String);

/// A TLE (Two-Line Element set) with an associated satellite identifier.
///
/// Implements [`HasId`] and [`HasTle`] (and therefore [`Satellite`][crate::Satellite])
/// so it can be passed directly to [`Predictor::new`][crate::Predictor::new].
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
#[derive(Debug, Clone)]
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

impl HasId for Tle {
    fn id(&self) -> &str {
        &self.satellite_name
    }
}

impl HasTle for Tle {
    fn line_1(&self) -> &str {
        &self.line_1
    }

    fn line_2(&self) -> &str {
        &self.line_2
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
/// Coordinates are in **degrees**; altitude is in **metres** above the WGS-84
/// ellipsoid.
///
/// # Example
///
/// ```
/// use sgp4_predict::GroundObserver;
///
/// // London, ~20 m ASL
/// let london = GroundObserver::new(51.5074, -0.1278, 20.0);
/// ```
#[derive(Debug, Clone)]
pub struct GroundObserver {
    /// Geodetic latitude in degrees (positive north).
    latitude_deg: f64,
    /// Geodetic longitude in degrees (positive east).
    longitude_deg: f64,
    /// Height above the WGS-84 ellipsoid in metres.
    altitude: f64,
}

impl GroundObserver {
    /// Construct a [`GroundObserver`].
    ///
    /// - `latitude_deg` — geodetic latitude in degrees (positive north)
    /// - `longitude_deg` — geodetic longitude in degrees (positive east)
    /// - `altitude` — height above the WGS-84 ellipsoid in metres
    pub const fn new(latitude_deg: f64, longitude_deg: f64, altitude: f64) -> Self {
        Self {
            latitude_deg,
            longitude_deg,
            altitude,
        }
    }
}

impl Observer for GroundObserver {
    fn latitude_deg(&self) -> f64 {
        self.latitude_deg
    }

    fn longitude_deg(&self) -> f64 {
        self.longitude_deg
    }

    fn altitude(&self) -> f64 {
        self.altitude
    }
}
