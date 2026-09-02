use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::vectors::PyVec3;

/// The satellite's view of a target on the ground.
///
/// `direction` is a unit vector in the spacecraft's LVLH frame — Z along nadir,
/// Y along the negative orbit normal, X along-track — which composes directly
/// with an antenna or instrument mounting rotation. Range is in metres, range
/// rate in m/s (positive = receding).
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct Pointing {
    pub direction: [f64; 3],
    #[pyo3(get)]
    pub range: f64,
    #[pyo3(get)]
    pub range_rate: f64,
}

impl Pointing {
    pub(crate) fn from_inner(p: sgp4_predict::Pointing) -> Self {
        Self {
            direction: [p.direction.x, p.direction.y, p.direction.z],
            range: p.range,
            range_rate: p.range_rate,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl Pointing {
    /// Unit vector from the satellite to the target, in the LVLH frame.
    #[getter]
    fn direction(&self) -> PyVec3 {
        PyVec3::new(self.direction[0], self.direction[1], self.direction[2])
    }

    /// Angle between `direction` and nadir, in degrees.
    ///
    /// Nadir is geocentric, the same convention as the `max_off_nadir` field of
    /// regard, so the two compare directly.
    #[getter]
    fn off_nadir_deg(&self) -> f64 {
        let [x, y, z] = self.direction;
        x.hypot(y).atan2(z).to_degrees()
    }

    fn __repr__(&self) -> String {
        format!(
            "Pointing(off_nadir={:.2}°, range={:.0}m, range_rate={:.1}m/s)",
            self.off_nadir_deg(),
            self.range,
            self.range_rate,
        )
    }
}
