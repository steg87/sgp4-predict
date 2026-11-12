use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};
use sgp4_predict::{KeplerianElements, MeanElements};

/// Osculating Keplerian elements at one instant. Angles are in degrees,
/// `[0, 360)` (inclination `[0, 180]`).
///
/// An equatorial orbit has `raan_deg == 0` and angles measured from the TEME
/// +X axis; a circular one has `argument_of_perigee_deg == 0`, so
/// `true_anomaly_deg` is then the argument of latitude.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct OsculatingElements {
    /// Semi-major axis in metres.
    #[pyo3(get)]
    pub semi_major_axis: f64,
    /// Eccentricity.
    #[pyo3(get)]
    pub eccentricity: f64,
    /// Inclination in degrees.
    #[pyo3(get)]
    pub inclination_deg: f64,
    /// Right ascension of the ascending node in degrees (TEME).
    #[pyo3(get)]
    pub raan_deg: f64,
    /// Argument of perigee in degrees.
    #[pyo3(get)]
    pub argument_of_perigee_deg: f64,
    /// True anomaly in degrees.
    #[pyo3(get)]
    pub true_anomaly_deg: f64,
    /// Mean anomaly in degrees.
    #[pyo3(get)]
    pub mean_anomaly_deg: f64,
    /// Mean motion in rad/s.
    #[pyo3(get)]
    pub mean_motion: f64,
    /// Orbital period in seconds.
    #[pyo3(get)]
    pub period_seconds: f64,
    /// Perigee height above the WGS-84 equatorial radius in metres.
    #[pyo3(get)]
    pub perigee_altitude: f64,
    /// Apogee height above the WGS-84 equatorial radius in metres.
    #[pyo3(get)]
    pub apogee_altitude: f64,
}

impl OsculatingElements {
    pub fn from_inner(k: KeplerianElements) -> Self {
        Self {
            semi_major_axis: k.semi_major_axis,
            eccentricity: k.eccentricity,
            inclination_deg: k.inclination.degrees(),
            raan_deg: k.raan.degrees(),
            argument_of_perigee_deg: k.argument_of_perigee.degrees(),
            true_anomaly_deg: k.true_anomaly.degrees(),
            mean_anomaly_deg: k.mean_anomaly.degrees(),
            mean_motion: k.mean_motion,
            period_seconds: k.period.num_microseconds().unwrap_or(0) as f64 / 1e6,
            perigee_altitude: k.perigee_altitude,
            apogee_altitude: k.apogee_altitude,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl OsculatingElements {
    fn __repr__(&self) -> String {
        format!(
            "OsculatingElements(a={:.0}m, e={:.6}, i={:.4}°, raan={:.4}°, argp={:.4}°, nu={:.4}°)",
            self.semi_major_axis,
            self.eccentricity,
            self.inclination_deg,
            self.raan_deg,
            self.argument_of_perigee_deg,
            self.true_anomaly_deg,
        )
    }
}

/// The mean elements the element set was published with. Angles are in
/// degrees.
///
/// `mean_motion` and `period_seconds` are the Kozai values used by TLEs and
/// OMM messages, so a semi-major axis derived from them differs by a few
/// kilometres from the Brouwer value SGP4 propagates internally.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct MeanElementSet {
    /// Element-set epoch (UTC).
    #[pyo3(get)]
    pub epoch: DateTime<Utc>,
    /// Inclination in degrees.
    #[pyo3(get)]
    pub inclination_deg: f64,
    /// Right ascension of the ascending node in degrees.
    #[pyo3(get)]
    pub raan_deg: f64,
    /// Eccentricity.
    #[pyo3(get)]
    pub eccentricity: f64,
    /// Argument of perigee in degrees.
    #[pyo3(get)]
    pub argument_of_perigee_deg: f64,
    /// Mean anomaly at `epoch` in degrees.
    #[pyo3(get)]
    pub mean_anomaly_deg: f64,
    /// Mean motion in rad/s.
    #[pyo3(get)]
    pub mean_motion: f64,
    /// Orbital period in seconds.
    #[pyo3(get)]
    pub period_seconds: f64,
    /// SGP4 drag term B*, in inverse earth radii.
    #[pyo3(get)]
    pub bstar: f64,
    /// First derivative of mean motion, as published (rev/day²).
    #[pyo3(get)]
    pub mean_motion_dot: f64,
    /// Second derivative of mean motion, as published (rev/day³).
    #[pyo3(get)]
    pub mean_motion_ddot: f64,
}

impl MeanElementSet {
    pub fn from_inner(m: MeanElements) -> Self {
        Self {
            epoch: m.epoch,
            inclination_deg: m.inclination.degrees(),
            raan_deg: m.raan.degrees(),
            eccentricity: m.eccentricity,
            argument_of_perigee_deg: m.argument_of_perigee.degrees(),
            mean_anomaly_deg: m.mean_anomaly.degrees(),
            mean_motion: m.mean_motion,
            period_seconds: m.period.num_microseconds().unwrap_or(0) as f64 / 1e6,
            bstar: m.bstar,
            mean_motion_dot: m.mean_motion_dot,
            mean_motion_ddot: m.mean_motion_ddot,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl MeanElementSet {
    fn __repr__(&self) -> String {
        format!(
            "MeanElementSet(epoch={}, i={:.4}°, e={:.7}, period={:.1}s)",
            self.epoch, self.inclination_deg, self.eccentricity, self.period_seconds,
        )
    }
}
