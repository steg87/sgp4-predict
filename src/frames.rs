use chrono::{DateTime, Utc};

use crate::{Observation, Observer, Position, StateVector, Velocity};

pub struct JulianDate(f64);
pub struct Radians(f64);

/// Marker struct for TEME frame
pub struct Teme;
pub type TemeState = StateVector<Teme>;

impl TemeState {
    /// Rotation about the Z-axis by GMST
    pub fn to_ecef(&self, t: DateTime<Utc>) -> EcefState {
        let (sin_g, cos_g) = gmst(julian_date(t)).0.sin_cos();

        EcefState::new(
            Position {
                x: cos_g * self.position.x + sin_g * self.position.y,
                y: -sin_g * self.position.x + cos_g * self.position.y,
                z: self.position.z,
            },
            Velocity {
                x: cos_g * self.velocity.x + sin_g * self.velocity.y,
                y: -sin_g * self.velocity.x + cos_g * self.velocity.y,
                z: self.velocity.z,
            },
        )
    }
}

/// Marker struct for ECEF frame
pub struct Ecef;
pub type EcefState = StateVector<Ecef>;

impl EcefState {
    pub fn to_enu(&self, observer: &impl Observer) -> EnuState {
        let obs_ecef = observer.to_ecef();
        let dp = self.position - obs_ecef.position;
        let dv = self.velocity - obs_ecef.velocity;

        let (sin_lat, cos_lat) = observer.latitude().sin_cos();
        let (sin_lon, cos_lon) = observer.longitude().sin_cos();

        EnuState::new(
            Position {
                x: -sin_lon * dp.x + cos_lon * dp.y,
                y: -sin_lat * cos_lon * dp.x - sin_lat * sin_lon * dp.y + cos_lat * dp.z,
                z: cos_lat * cos_lon * dp.x + cos_lat * sin_lon * dp.y + sin_lat * dp.z,
            },
            Velocity {
                x: -sin_lon * dv.x + cos_lon * dv.y,
                y: -sin_lat * cos_lon * dv.x - sin_lat * sin_lon * dv.y + cos_lat * dv.z,
                z: cos_lat * cos_lon * dv.x + cos_lat * sin_lon * dv.y + sin_lat * dv.z,
            },
        )
    }
}

/// Marker struct for ECEF frame
pub struct Enu;
pub type EnuState = StateVector<Enu>;

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
    pub(crate) fn to_elevation(&self) -> (f64, f64) {
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
    let unix_seconds = t.timestamp() as f64;
    JulianDate(unix_seconds / 86400.0 + 2440587.5)
}

fn gmst(jd: JulianDate) -> Radians {
    let t = (jd.0 - 2451545.0) / 36525.0;

    let gmst_sec = 67310.54841 + (876600.0 * 3600.0 + 8640184.812866) * t + 0.093104 * t * t
        - 6.2e-6 * t * t * t;

    // Convert seconds → radians
    Radians(((gmst_sec % 86400.0) * std::f64::consts::TAU) / 86400.0)
}
