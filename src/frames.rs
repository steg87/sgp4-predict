use chrono::{DateTime, Utc};

use crate::{
    Observation, Observer,
    vectors::{Position, StateVector, Velocity},
};

pub struct JulianDate(f64);
pub struct Radians(f64);

/// TEME state vector
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

        let (sin_g, cos_g) = gmst(julian_date(t)).0.sin_cos();

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

/// ECEF state vector
pub type EcefState = StateVector<markers::Ecef>;

impl EcefState {
    pub fn to_enu(&self, observer: &impl Observer) -> EnuState {
        let obs_ecef = observer.to_ecef();
        let dp = self.position - obs_ecef.position;
        let dv = self.velocity - obs_ecef.velocity;

        let (sin_lat, cos_lat) = observer.latitude().sin_cos();
        let (sin_lon, cos_lon) = observer.longitude().sin_cos();

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

/// ENU state vector
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
            azimuth,
            elevation,
            range,
            range_rate,
        }
    }

    /// Convert to an (elevation, elevation rate)
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

fn julian_date(t: DateTime<Utc>) -> JulianDate {
    let unix_seconds = t.timestamp() as f64 + t.timestamp_subsec_nanos() as f64 * 1e-9;
    JulianDate(unix_seconds / 86400.0 + 2440587.5)
}

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
