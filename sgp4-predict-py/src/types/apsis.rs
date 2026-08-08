use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};

/// Whether an apsis event is apogee or perigee.
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
pub enum ApsisEvent {
    /// Point of greatest distance from Earth (maximum altitude).
    Apogee,
    /// Point of closest approach to Earth (minimum altitude).
    Perigee,
}

/// A detected apsis event with refined time and altitude.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct Apsis {
    pub time: DateTime<Utc>,
    pub event: ApsisEvent,
    pub altitude: f64,
}

#[gen_stub_pymethods]
#[pymethods]
impl Apsis {
    /// Time of the apsis event (UTC).
    #[getter]
    fn time(&self) -> DateTime<Utc> {
        self.time
    }

    /// Whether this is an apogee or perigee.
    #[getter]
    fn event(&self) -> ApsisEvent {
        self.event
    }

    /// Altitude above the WGS-84 equatorial radius in metres.
    #[getter]
    fn altitude(&self) -> f64 {
        self.altitude
    }

    fn __repr__(&self) -> String {
        format!(
            "Apsis(event={:?}, time={}, altitude={:.0}m)",
            self.event, self.time, self.altitude,
        )
    }
}
