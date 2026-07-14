use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};

/// Whether a pole-approach event is a northern or southern approach.
#[gen_stub_pyclass_enum]
#[pyclass(
    eq,
    eq_int,
    hash,
    frozen,
    from_py_object,
    module = "sgp4_predict._sgp4_predict"
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PoleEvent {
    /// Closest approach to the North Pole (maximum latitude).
    North,
    /// Closest approach to the South Pole (minimum latitude).
    South,
}

/// A detected pole-approach event with refined time and latitude.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
pub struct PoleApproach {
    pub time: DateTime<Utc>,
    pub event: PoleEvent,
    pub latitude: f64,
}

#[gen_stub_pymethods]
#[pymethods]
impl PoleApproach {
    /// Time of closest approach (UTC).
    #[getter]
    fn time(&self) -> DateTime<Utc> {
        self.time
    }

    /// Whether this is a northern or southern approach.
    #[getter]
    fn event(&self) -> PoleEvent {
        self.event
    }

    /// Geocentric latitude in radians (`asin(z / |r|)`), positive north.
    #[getter]
    fn latitude(&self) -> f64 {
        self.latitude
    }

    /// Geocentric latitude in degrees, positive north.
    fn latitude_deg(&self) -> f64 {
        self.latitude.to_degrees()
    }

    fn __repr__(&self) -> String {
        format!(
            "PoleApproach(event={:?}, time={}, latitude={:.3}deg)",
            self.event,
            self.time,
            self.latitude_deg(),
        )
    }
}
