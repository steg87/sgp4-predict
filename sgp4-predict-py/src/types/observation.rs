use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// A point observation of a satellite from a ground location.
///
/// Use `azimuth_deg` and `elevation_deg` for angular values.
/// Range is in metres, range rate in m/s (positive = receding).
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct Observation {
    pub azimuth: f64,
    pub elevation: f64,
    #[pyo3(get)]
    pub range: f64,
    #[pyo3(get)]
    pub range_rate: f64,
}

impl Observation {
    pub(crate) fn from_inner(obs: sgp4_predict::Observation) -> Self {
        Self {
            azimuth: obs.azimuth.to_f64(),
            elevation: obs.elevation.to_f64(),
            range: obs.range,
            range_rate: obs.range_rate,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl Observation {
    /// Azimuth in degrees from north, measured clockwise, in `(-180, 180]`.
    #[getter]
    fn azimuth_deg(&self) -> f64 {
        self.azimuth.to_degrees()
    }

    /// Elevation above the horizon in degrees.
    #[getter]
    fn elevation_deg(&self) -> f64 {
        self.elevation.to_degrees()
    }

    fn __repr__(&self) -> String {
        format!(
            "Observation(az={:.2}°, el={:.2}°, range={:.0}m, range_rate={:.1}m/s)",
            self.azimuth.to_degrees(),
            self.elevation.to_degrees(),
            self.range,
            self.range_rate,
        )
    }
}
