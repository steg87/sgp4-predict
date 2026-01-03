use chrono::{DateTime, Utc};

use crate::{Observation, Observer, Position, StateVector, Velocity, units::SI};

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
            Position::from_si(
                cos_g * self.position.x.to_si() + sin_g * self.position.y.to_si(),
                -sin_g * self.position.x.to_si() + cos_g * self.position.y.to_si(),
                self.position.z.to_si(),
            ),
            Velocity::from_si(
                cos_g * self.velocity.x.to_si() + sin_g * self.velocity.y.to_si(),
                -sin_g * self.velocity.x.to_si() + cos_g * self.velocity.y.to_si(),
                self.velocity.z.to_si(),
            ),
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

        let (sin_lat, cos_lat) = observer.latitude().to_si().sin_cos();
        let (sin_lon, cos_lon) = observer.longitude().to_si().sin_cos();

        EnuState::new(
            Position::from_si(
                -sin_lon * dp.x.to_si() + cos_lon * dp.y.to_si(),
                -sin_lat * cos_lon * dp.x.to_si() - sin_lat * sin_lon * dp.y.to_si()
                    + cos_lat * dp.z.to_si(),
                cos_lat * cos_lon * dp.x.to_si()
                    + cos_lat * sin_lon * dp.y.to_si()
                    + sin_lat * dp.z.to_si(),
            ),
            Velocity::from_si(
                -sin_lon * dv.x.to_si() + cos_lon * dv.y.to_si(),
                -sin_lat * cos_lon * dv.x.to_si() - sin_lat * sin_lon * dv.y.to_si()
                    + cos_lat * dv.z.to_si(),
                cos_lat * cos_lon * dv.x.to_si()
                    + cos_lat * sin_lon * dv.y.to_si()
                    + sin_lat * dv.z.to_si(),
            ),
        )
    }
}

/// Marker struct for ECEF frame
pub struct Enu;
pub type EnuState = StateVector<Enu>;

impl EnuState {
    pub fn to_observation(&self) -> Observation {
        let (e, n, u) = (
            self.position.x.to_si(),
            self.position.y.to_si(),
            self.position.z.to_si(),
        );
        let (ed, nd, ud) = (
            self.velocity.x.to_si(),
            self.velocity.y.to_si(),
            self.velocity.z.to_si(),
        );

        let range = (e * e + n * n + u * u).sqrt();
        let range_rate = (e * ed + n * nd + u * ud) / range;
        let az = e.atan2(n);
        let el = (u / range).asin();

        Observation::from_si(az, el, range, range_rate)
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
