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
    angle::Radians,
    observe::ObserverExt,
    vectors::{Position, StateVector, Velocity},
};

/// Earth's equatorial radius (WGS-84), metres.
pub(crate) const WGS84_A: f64 = 6_378_137.0;

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
    use super::{EnuState, gmst, julian_date, sun_position_eci};
    use crate::vectors::{Position, Velocity};
    use chrono::{TimeZone, Utc};

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
