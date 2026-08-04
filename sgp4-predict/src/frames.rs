//! Coordinate frame types and the conversions between them.
//!
//! Three frames are used in the prediction pipeline:
//!
//! - **TEME** ([`TemeState`]): True Equator Mean Equinox — the native SGP4 output frame.
//! - **ECEF** ([`EcefState`]): Earth-Centred Earth-Fixed — rotates with Earth.
//! - **ENU** ([`EnuState`]): East-North-Up — local frame relative to a ground observer.
//!
//! The normal conversion chain is `TEME → ECEF → ENU → [`Observation`]`.
//!
//! [`Observation`]: crate::Observation

use chrono::{DateTime, Utc};

use crate::{
    Observation, Observer,
    angle::{Degrees, Radians},
    observe::ObserverExt,
    vectors::{Position, StateVector, Velocity},
};

/// Earth's equatorial radius (WGS-84), metres.
pub(crate) const WGS84_A: f64 = 6_378_137.0;

/// WGS-84 flattening.
pub(crate) const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// WGS-84 first eccentricity squared, `f(2 − f)`.
pub(crate) const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

pub struct JulianDate(f64);

/// State vector in the True Equator Mean Equinox (TEME) frame — the native SGP4 output frame.
pub type TemeState = StateVector<markers::Teme>;

impl TemeState {
    /// Convert TEME state to ECEF by rotating about the Z-axis by GMST.
    ///
    /// Position: `r_ECEF = R(θ) · r_TEME`
    ///
    /// Velocity requires an extra term because ECEF is a rotating frame.
    /// Differentiating `r_ECEF = R(θ) · r_TEME` with respect to time gives:
    ///   `v_ECEF = R(θ) · v_TEME + ω_Earth × r_ECEF`
    ///
    /// where `ω_Earth = [0, 0, ω_E]`. Expanding the cross product:
    ///   `ω_Earth × r_ECEF = [ω_E · ry, -ω_E · rx, 0]`
    pub fn to_ecef(&self, t: DateTime<Utc>) -> EcefState {
        // Earth's sidereal rotation rate (rad/s), WGS-84
        const OMEGA_EARTH: f64 = 7.292_115_0e-5;

        let (sin_g, cos_g) = gmst(julian_date(t)).to_f64().sin_cos();

        // Rotate position into ECEF: r_ECEF = R(θ) · r_TEME
        let rx = cos_g * self.position.x + sin_g * self.position.y;
        let ry = -sin_g * self.position.x + cos_g * self.position.y;
        let rz = self.position.z;

        // Rotate velocity into ECEF, then add the frame-drag term ω_Earth × r_ECEF
        let vx_rot = cos_g * self.velocity.x + sin_g * self.velocity.y;
        let vy_rot = -sin_g * self.velocity.x + cos_g * self.velocity.y;

        StateVector::new(
            Position::new(rx, ry, rz),
            Velocity::new(
                vx_rot + OMEGA_EARTH * ry,
                vy_rot - OMEGA_EARTH * rx,
                self.velocity.z,
            ),
        )
    }
}

/// State vector in the Earth-Centred Earth-Fixed (ECEF) frame.
pub type EcefState = StateVector<markers::Ecef>;

impl EcefState {
    /// Convert ECEF state to ENU relative to an observer.
    ///
    /// Subtracts the observer's ECEF position (derived from geodetic
    /// coordinates via the WGS-84 ellipsoid) and rotates into the local
    /// East-North-Up frame at the observer's location.
    pub fn to_enu(&self, observer: &impl Observer) -> EnuState {
        let obs_ecef = observer.to_ecef();
        let dp = self.position - obs_ecef.position;
        let dv = self.velocity - obs_ecef.velocity;

        let (sin_lat, cos_lat) = observer.latitude().to_radians().to_f64().sin_cos();
        let (sin_lon, cos_lon) = observer.longitude().to_radians().to_f64().sin_cos();

        StateVector::new(
            Position::new(
                -sin_lon * dp.x + cos_lon * dp.y,
                -sin_lat * cos_lon * dp.x - sin_lat * sin_lon * dp.y + cos_lat * dp.z,
                cos_lat * cos_lon * dp.x + cos_lat * sin_lon * dp.y + sin_lat * dp.z,
            ),
            Velocity::new(
                -sin_lon * dv.x + cos_lon * dv.y,
                -sin_lat * cos_lon * dv.x - sin_lat * sin_lon * dv.y + cos_lat * dv.z,
                cos_lat * cos_lon * dv.x + cos_lat * sin_lon * dv.y + sin_lat * dv.z,
            ),
        )
    }

    /// Convert the ECEF position to geodetic latitude, longitude and height
    /// above the WGS-84 ellipsoid. Velocity is discarded.
    ///
    /// Applied to a propagated satellite state this gives the sub-satellite
    /// point; see [`Predictor::sub_point`].
    ///
    /// [`Predictor::sub_point`]: crate::Predictor::sub_point
    pub fn to_geodetic(&self) -> Geodetic {
        geodetic_from_ecef(self.position.x, self.position.y, self.position.z)
    }
}

/// A point on Earth's surface, named rather than positional so latitude and
/// longitude cannot be transposed.
///
/// ```
/// use sgp4_predict::{Degrees, LatLon};
///
/// let glasgow = LatLon {
///     latitude: Degrees(55.8642),
///     longitude: Degrees(-4.2518),
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    /// Geodetic latitude (positive north).
    pub latitude: Degrees,
    /// Geodetic longitude (positive east).
    pub longitude: Degrees,
}

impl LatLon {
    pub const fn new(latitude: Degrees, longitude: Degrees) -> Self {
        Self {
            latitude,
            longitude,
        }
    }
}

impl From<Geodetic> for LatLon {
    fn from(g: Geodetic) -> Self {
        Self {
            latitude: g.latitude,
            longitude: g.longitude,
        }
    }
}

/// Latitude first, then longitude — the same order as [`LatLon::new`].
impl From<(Degrees, Degrees)> for LatLon {
    fn from((latitude, longitude): (Degrees, Degrees)) -> Self {
        Self {
            latitude,
            longitude,
        }
    }
}

/// A geodetic position on or above the WGS-84 ellipsoid.
///
/// Altitude is in **metres**. Longitude is in `(-180, 180]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geodetic {
    /// Geodetic latitude (positive north).
    pub latitude: Degrees,
    /// Geodetic longitude (positive east).
    pub longitude: Degrees,
    /// Height above the WGS-84 ellipsoid in metres.
    pub altitude: f64,
}

impl Observer for Geodetic {
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

/// Vermeille's (2002) closed-form inverse of the geodetic-to-ECEF transform.
///
/// Chosen over Bowring's single iteration because it stays exact at orbital
/// altitude, which is where Bowring degrades. Degenerate only within about
/// 43 km of Earth's centre, which is guarded below.
fn geodetic_from_ecef(x: f64, y: f64, z: f64) -> Geodetic {
    const E4: f64 = WGS84_E2 * WGS84_E2;

    let xy2 = x * x + y * y;
    let p = xy2 / (WGS84_A * WGS84_A);
    let q = (1.0 - WGS84_E2) * z * z / (WGS84_A * WGS84_A);
    let r = (p + q - E4) / 6.0;

    // Inside the ellipsoid's evolute the cube root below is not defined. No
    // orbit reaches it; return a well-formed value rather than NaN. The
    // signature cannot report the failure, and 0°/0° is the one answer
    // indistinguishable from a real one — only the -a altitude marks it.
    if r <= 0.0 {
        return Geodetic {
            latitude: Degrees(0.0),
            longitude: Degrees(0.0),
            altitude: -WGS84_A,
        };
    }

    let s = E4 * p * q / (4.0 * r * r * r);
    let t = (1.0 + s + (s * (2.0 + s)).sqrt()).cbrt();
    let u = r * (1.0 + t + 1.0 / t);
    let v = (u * u + E4 * q).sqrt();
    let w = WGS84_E2 * (u + v - q) / (2.0 * v);
    let k = (u + v + w * w).sqrt() - w;

    let d = k * xy2.sqrt() / (k + WGS84_E2);
    let dz = (d * d + z * z).sqrt();

    Geodetic {
        // The half-angle form is exact at both poles, where `d` is zero.
        latitude: Radians(2.0 * z.atan2(d + dz)).to_degrees(),
        longitude: Radians(y.atan2(x)).to_degrees(),
        altitude: (k + WGS84_E2 - 1.0) / k * dz,
    }
}

/// State vector in the East-North-Up (ENU) frame, relative to a ground observer.
pub type EnuState = StateVector<markers::Enu>;

impl EnuState {
    /// Convert to an observation (range, range rate, azimuth, elevation)
    pub fn to_observation(&self) -> Observation {
        let (e, n, u) = (self.position.x, self.position.y, self.position.z);
        let (ed, nd, ud) = (self.velocity.x, self.velocity.y, self.velocity.z);

        let range = (e * e + n * n + u * u).sqrt();
        let range_rate = (e * ed + n * nd + u * ud) / range;
        let azimuth = e.atan2(n);
        let elevation = (u / range).asin();

        Observation {
            azimuth: Radians(azimuth),
            elevation: Radians(elevation),
            range,
            range_rate,
        }
    }

    /// Return `(elevation, elevation_rate)` in radians and radians per second.
    ///
    /// Used internally as the derivative function for Newton-Raphson refinement
    /// of transit crossing times.
    pub(crate) fn elevation_and_rate(&self) -> (f64, f64) {
        let (e, n, u) = (self.position.x, self.position.y, self.position.z);
        let (ed, nd, ud) = (self.velocity.x, self.velocity.y, self.velocity.z);

        let horiz2 = e * e + n * n;
        let horiz = horiz2.sqrt();
        let range2 = horiz2 + u * u;

        let el = u.atan2(horiz);

        let el_rate = if horiz > 1e-12 {
            (horiz2 * ud - u * (e * ed + n * nd)) / (range2 * horiz)
        } else {
            // At or near zenith elevation rate undefined
            0.0 // Treat as zero
        };

        (el, el_rate)
    }
}

/// Convert a UTC instant to a Julian Date.
///
/// The Unix epoch (1970-01-01T00:00:00Z) corresponds to JD 2440587.5.
/// Subsecond precision is preserved by including the nanosecond component.
fn julian_date(t: DateTime<Utc>) -> JulianDate {
    let unix_seconds = t.timestamp() as f64 + t.timestamp_subsec_nanos() as f64 * 1e-9;
    JulianDate(unix_seconds / 86400.0 + 2440587.5)
}

/// Compute Greenwich Mean Sidereal Time (GMST) from a Julian Date.
///
/// Uses the IAU 1982 polynomial (Aoki et al. 1982), which expresses GMST in
/// seconds of time as a cubic in Julian centuries `T` since J2000.0
/// (JD 2451545.0):
///
/// ```text
/// GMST [s] = 67310.54841
///          + (876600 × 3600 + 8640184.812866) T
///          + 0.093104 T²
///          − 6.2×10⁻⁶ T³
/// ```
///
/// The result is reduced to `[0, 2π)` and returned in radians.
fn gmst(jd: JulianDate) -> Radians {
    let t = (jd.0 - 2451545.0) / 36525.0;

    let gmst_sec = 67310.54841 + (876600.0 * 3600.0 + 8640184.812866) * t + 0.093104 * t * t
        - 6.2e-6 * t * t * t;

    // Convert seconds → radians.
    // Use two-step modulo to guard against negative gmst_sec (dates before J2000): Rust's %
    // preserves the sign of the dividend, so a single `% 86400.0` can produce a negative
    // remainder. Adding 86400.0 before the second `%` ensures the result is always in [0, 86400).
    Radians((((gmst_sec % 86400.0) + 86400.0) % 86400.0) * std::f64::consts::TAU / 86400.0)
}

/// Compute the position of the Sun in the geocentric equatorial (ECI) frame.
///
/// Uses the low-precision algorithm from the Astronomical Almanac, accurate to
/// approximately 0.01° over 1950–2050. Sufficient for satellite illumination and
/// shadow calculations.
///
/// Returns `[x, y, z]` in **metres** with origin at Earth's centre. Axes are
/// aligned with the J2000 mean equator (ECI ≈ TEME to this precision).
pub(crate) fn sun_position_eci(t: DateTime<Utc>) -> [f64; 3] {
    const AU_M: f64 = 1.495_978_707e11; // 1 AU in metres

    let n = julian_date(t).0 - 2_451_545.0; // days from J2000.0

    // Mean longitude and mean anomaly (degrees)
    let l_deg = 280.460 + 0.985_647_4 * n;
    let g_deg = 357.528 + 0.985_600_3 * n;
    let g = g_deg.to_radians();

    // Ecliptic longitude (radians)
    let lambda = (l_deg + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();

    // Sun–Earth distance in AU
    let r_au = 1.000_14 - 0.016_71 * g.cos() - 0.000_14 * (2.0 * g).cos();

    // Mean obliquity of the ecliptic (radians)
    let eps = (23.439 - 0.000_000_4 * n).to_radians();

    [
        r_au * AU_M * lambda.cos(),
        r_au * AU_M * eps.cos() * lambda.sin(),
        r_au * AU_M * eps.sin() * lambda.sin(),
    ]
}

mod markers {
    /// Marker struct for TEME frame
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Teme;

    /// Marker struct for ECEF frame
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Ecef;

    /// Marker struct for ENU frame
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Enu;
}

#[cfg(test)]
mod tests {
    use super::{EcefState, EnuState, Geodetic, WGS84_A, gmst, julian_date, sun_position_eci};
    use crate::angle::Degrees;
    use crate::observe::ObserverExt;
    use crate::vectors::{Position, Velocity};
    use chrono::{TimeZone, Utc};

    // --- EcefState::to_geodetic ---

    /// Round-trip geodetic → ECEF → geodetic. `to_ecef` is the forward
    /// transform this is the closed-form inverse of, so agreement to
    /// sub-millimetre is the correctness bar.
    #[test]
    fn test_to_geodetic_round_trip() {
        let heights = [-500.0, 0.0, 800_000.0, 35_786_000.0];
        for &h in &heights {
            for lat_deg in [-90.0, -89.9, -45.0, -0.1, 0.0, 23.5, 60.0, 89.9, 90.0] {
                for lon_deg in [-180.0, -179.9, -90.0, -0.1, 0.0, 45.0, 179.9] {
                    let start = Geodetic {
                        latitude: Degrees(lat_deg),
                        longitude: Degrees(lon_deg),
                        altitude: h,
                    };
                    let back = start.to_ecef().to_geodetic();

                    assert!(
                        (back.latitude.to_f64() - lat_deg).abs() < 1e-9,
                        "latitude {lat_deg} h={h} round-tripped to {}",
                        back.latitude
                    );
                    assert!(
                        (back.altitude - h).abs() < 1e-3,
                        "altitude {h} at lat {lat_deg} round-tripped to {}",
                        back.altitude
                    );

                    // Longitude is undefined at the poles, where the forward
                    // transform collapses x and y to zero.
                    if lat_deg.abs() < 90.0 {
                        assert!(
                            (back.longitude.to_f64() - lon_deg).abs() < 1e-9,
                            "longitude {lon_deg} at lat {lat_deg} round-tripped to {}",
                            back.longitude
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_to_geodetic_known_points() {
        // On the equator at the prime meridian the ECEF position is exactly
        // the semi-major axis along +x.
        let g = EcefState::new(
            Position::new(WGS84_A, 0.0, 0.0),
            Velocity::new(0.0, 0.0, 0.0),
        )
        .to_geodetic();
        assert!(g.latitude.to_f64().abs() < 1e-12);
        assert!(g.longitude.to_f64().abs() < 1e-12);
        assert!(
            g.altitude.abs() < 1e-6,
            "altitude {} should be 0",
            g.altitude
        );

        // On the polar axis at the semi-minor axis: latitude 90°, height 0.
        let b = WGS84_A * (1.0 - super::WGS84_F);
        let g =
            EcefState::new(Position::new(0.0, 0.0, b), Velocity::new(0.0, 0.0, 0.0)).to_geodetic();
        assert!((g.latitude.to_f64() - 90.0).abs() < 1e-9);
        assert!(
            g.altitude.abs() < 1e-6,
            "altitude {} should be 0",
            g.altitude
        );
    }

    #[test]
    fn test_to_geodetic_earth_centre_does_not_produce_nan() {
        // Degenerate input the closed form is not defined for. Never reached by
        // a real orbit, but must not yield NaN.
        let g = EcefState::new(Position::new(0.0, 0.0, 0.0), Velocity::new(0.0, 0.0, 0.0))
            .to_geodetic();
        assert!(g.latitude.to_f64().is_finite());
        assert!(g.longitude.to_f64().is_finite());
        assert!(g.altitude.is_finite());
    }

    // --- julian_date ---

    #[test]
    fn test_julian_date_j2000() {
        // J2000.0 is defined as 2000-01-01T12:00:00 UTC = JD 2451545.0.
        // Verifies the Unix-epoch offset constant (2440587.5) and the
        // seconds-to-days divisor.
        let t = Utc.with_ymd_and_hms(2000, 1, 1, 12, 0, 0).unwrap();
        let jd = julian_date(t);
        assert!(
            (jd.0 - 2451545.0).abs() < 1e-9,
            "JD at J2000.0 = {}, expected 2451545.0",
            jd.0
        );
    }

    // --- gmst ---

    #[test]
    fn test_gmst_j2000_constant_term() {
        // At J2000.0 T = 0, so the polynomial collapses to its constant term:
        //   GMST_sec = 67310.54841
        //   GMST_rad = 67310.54841 × 2π / 86400 ≈ 4.894961...
        // Catches coefficient-transcription errors in the IAU 1982 polynomial.
        let t = Utc.with_ymd_and_hms(2000, 1, 1, 12, 0, 0).unwrap();
        let g = gmst(julian_date(t));
        let expected = 67310.54841 * std::f64::consts::TAU / 86400.0;
        assert!(
            (g.to_f64() - expected).abs() < 1e-9,
            "GMST at J2000.0 = {:.9}, expected {:.9}",
            g.to_f64(),
            expected
        );
    }

    #[test]
    fn test_gmst_always_in_range() {
        // GMST must always be in [0, 2π) regardless of date.
        // The negative-modulo guard is tested with a pre-J2000 date.
        let dates = [
            Utc.with_ymd_and_hms(1990, 1, 1, 0, 0, 0).unwrap(), // T < 0
            Utc.with_ymd_and_hms(2000, 1, 1, 12, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 6, 21, 0, 0, 0).unwrap(),
        ];
        for t in &dates {
            let g = gmst(julian_date(*t));
            assert!(
                g.to_f64() >= 0.0 && g.to_f64() < std::f64::consts::TAU,
                "GMST {:.6} out of [0, 2π) for {t:?}",
                g.to_f64()
            );
        }
    }

    // --- sun_position_eci ---

    #[test]
    fn test_sun_position_approx_one_au() {
        // Sun–Earth distance must be approximately 1 AU (±2 %) year-round.
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 0, 0, 0).unwrap();
        let sun = sun_position_eci(t);
        let r = (sun[0].powi(2) + sun[1].powi(2) + sun[2].powi(2)).sqrt();
        let au = 1.495_978_707e11_f64;
        assert!(
            (r / au - 1.0).abs() < 0.02,
            "Sun distance {r:.3e} m deviates > 2 % from 1 AU"
        );
    }

    #[test]
    fn test_sun_position_solstice_z_sign() {
        // At northern summer solstice the Sun is north of the equatorial plane (z > 0);
        // at northern winter solstice it is south (z < 0).
        let summer = Utc.with_ymd_and_hms(2024, 6, 21, 0, 0, 0).unwrap();
        assert!(
            sun_position_eci(summer)[2] > 0.0,
            "Sun z should be positive at northern summer solstice"
        );

        let winter = Utc.with_ymd_and_hms(2024, 12, 21, 0, 0, 0).unwrap();
        assert!(
            sun_position_eci(winter)[2] < 0.0,
            "Sun z should be negative at northern winter solstice"
        );
    }

    // --- EnuState::elevation_and_rate ---

    #[test]
    fn test_elevation_and_rate_45_degrees() {
        // ENU position (r, 0, r): satellite is due east at 45° elevation.
        // Stationary → el_rate must be zero.
        let r = 1_000_000.0_f64;
        let sv = EnuState::new(Position::new(r, 0.0, r), Velocity::new(0.0, 0.0, 0.0));
        let (el, el_rate) = sv.elevation_and_rate();
        assert!(
            (el - std::f64::consts::FRAC_PI_4).abs() < 1e-12,
            "elevation should be π/4, got {el}"
        );
        assert_eq!(el_rate, 0.0);
    }

    #[test]
    fn test_elevation_and_rate_ascending() {
        // Satellite on the eastern horizon, moving upward → el_rate > 0.
        let sv = EnuState::new(
            Position::new(1_000_000.0, 0.0, 0.0),
            Velocity::new(0.0, 0.0, 1_000.0),
        );
        let (_, el_rate) = sv.elevation_and_rate();
        assert!(
            el_rate > 0.0,
            "el_rate should be positive for ascending satellite"
        );
    }

    #[test]
    fn test_elevation_and_rate_near_zenith_branch() {
        // horiz² = e² + n² ≈ 0 → hits the near-zenith guard branch and returns
        // el_rate = 0.0 instead of dividing by zero.
        let sv = EnuState::new(
            Position::new(0.0, 0.0, 800_000.0), // straight overhead
            Velocity::new(100.0, 0.0, 0.0),     // moving east
        );
        let (el, el_rate) = sv.elevation_and_rate();
        assert!(
            (el - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
            "elevation should be π/2 overhead, got {el}"
        );
        assert_eq!(el_rate, 0.0, "el_rate should be 0 in near-zenith branch");
    }
}
