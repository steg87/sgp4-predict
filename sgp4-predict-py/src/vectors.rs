use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use sgp4_predict::{EcefState, EnuState, LvlhState, TemeState};

use crate::area::GeodeticPoint;
use crate::types::{Observation, Pointing};

/// A 3-component vector (x, y, z).  Used for position (metres) and velocity (m/s).
#[gen_stub_pyclass]
#[pyclass(eq, frozen, name = "Vec3", module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct PyVec3 {
    #[pyo3(get)]
    pub x: f64,
    #[pyo3(get)]
    pub y: f64,
    #[pyo3(get)]
    pub z: f64,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyVec3 {
    #[new]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn __repr__(&self) -> String {
        format!("Vec3(x={}, y={}, z={})", self.x, self.y, self.z)
    }
}

/// State vector in the True Equator Mean Equinox (TEME) frame — the native SGP4 output frame.
/// Positions in metres, velocities in m/s.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct StateVectorTeme {
    pub(crate) inner: TemeState,
}

impl StateVectorTeme {
    pub fn from_inner(inner: TemeState) -> Self {
        Self { inner }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl StateVectorTeme {
    /// Position in the TEME frame (metres).
    #[getter]
    fn position(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.position.x,
            y: self.inner.position.y,
            z: self.inner.position.z,
        }
    }

    /// Velocity in the TEME frame (m/s).
    #[getter]
    fn velocity(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.velocity.x,
            y: self.inner.velocity.y,
            z: self.inner.velocity.z,
        }
    }

    /// Convert to the Earth-Centred Earth-Fixed (ECEF) frame at the given UTC time.
    fn to_ecef(&self, t: DateTime<Utc>) -> StateVectorEcef {
        StateVectorEcef {
            inner: self.inner.to_ecef(t),
        }
    }

    /// Express `target` relative to this satellite, in the satellite's LVLH frame.
    ///
    /// The receiver is the origin here — the mirror of `to_enu`, where the
    /// receiver is the satellite and the argument supplies the origin.
    fn to_lvlh(&self, target: &StateVectorTeme) -> StateVectorLvlh {
        StateVectorLvlh {
            inner: self.inner.to_lvlh(&target.inner),
        }
    }
}

/// State vector in the Earth-Centred Earth-Fixed (ECEF) frame.
/// Positions in metres, velocities in m/s.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct StateVectorEcef {
    pub(crate) inner: EcefState,
}

#[gen_stub_pymethods]
#[pymethods]
impl StateVectorEcef {
    /// Position in the ECEF frame (metres).
    #[getter]
    fn position(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.position.x,
            y: self.inner.position.y,
            z: self.inner.position.z,
        }
    }

    /// Velocity in the ECEF frame (m/s).
    #[getter]
    fn velocity(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.velocity.x,
            y: self.inner.velocity.y,
            z: self.inner.velocity.z,
        }
    }

    /// Convert to the East-North-Up (ENU) frame relative to the given observer.
    fn to_enu(&self, observer: &GeodeticPoint) -> StateVectorEnu {
        StateVectorEnu {
            inner: self.inner.to_enu(observer.inner),
        }
    }

    /// Convert back to the TEME frame, the inverse of `StateVectorTeme.to_ecef`.
    fn to_teme(&self, t: DateTime<Utc>) -> StateVectorTeme {
        StateVectorTeme {
            inner: self.inner.to_teme(t),
        }
    }
}

/// State vector in the East-North-Up (ENU) frame relative to a ground observer.
/// Positions in metres, velocities in m/s.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct StateVectorEnu {
    pub(crate) inner: EnuState,
}

#[gen_stub_pymethods]
#[pymethods]
impl StateVectorEnu {
    /// Position in the ENU frame (metres).
    #[getter]
    fn position(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.position.x,
            y: self.inner.position.y,
            z: self.inner.position.z,
        }
    }

    /// Velocity in the ENU frame (m/s).
    #[getter]
    fn velocity(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.velocity.x,
            y: self.inner.velocity.y,
            z: self.inner.velocity.z,
        }
    }

    /// Convert to a ground observation (azimuth, elevation, range, range_rate).
    fn to_observation(&self) -> Observation {
        let obs = self.inner.to_observation();
        Observation {
            azimuth: obs.azimuth.to_f64(),
            elevation: obs.elevation.to_f64(),
            range: obs.range,
            range_rate: obs.range_rate,
        }
    }
}

/// State vector in the satellite's LVLH frame: Z along nadir, Y along the
/// negative orbit normal, X completing the right-handed triad along-track.
/// Positions in metres, velocities in m/s.
///
/// The velocity is the inertial relative velocity resolved on the LVLH axes,
/// not the derivative of the LVLH position, so `range_rate` is Doppler-correct.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct StateVectorLvlh {
    pub(crate) inner: LvlhState,
}

#[gen_stub_pymethods]
#[pymethods]
impl StateVectorLvlh {
    /// Position in the LVLH frame (metres).
    #[getter]
    fn position(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.position.x,
            y: self.inner.position.y,
            z: self.inner.position.z,
        }
    }

    /// Velocity in the LVLH frame (m/s).
    #[getter]
    fn velocity(&self) -> PyVec3 {
        PyVec3 {
            x: self.inner.velocity.x,
            y: self.inner.velocity.y,
            z: self.inner.velocity.z,
        }
    }

    /// Reduce to the direction, range and range rate of the target.
    fn to_pointing(&self) -> Pointing {
        Pointing::from_inner(self.inner.to_pointing())
    }
}
