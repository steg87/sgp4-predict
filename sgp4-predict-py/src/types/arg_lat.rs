use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};

/// Which of the two argument-of-latitude points an event is.
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
pub enum ArgLatEvent {
    /// The satellite reaches the requested argument of latitude.
    Argument,
    /// The satellite reaches the opposite point, 180° round the orbit.
    Antipode,
}

/// A detected argument-of-latitude event with its refined time.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq)]
pub struct ArgLatCrossing {
    pub time: DateTime<Utc>,
    pub event: ArgLatEvent,
}

#[gen_stub_pymethods]
#[pymethods]
impl ArgLatCrossing {
    /// Time of the event (UTC).
    #[getter]
    fn time(&self) -> DateTime<Utc> {
        self.time
    }

    /// Whether this is the requested argument or the opposite point.
    #[getter]
    fn event(&self) -> ArgLatEvent {
        self.event
    }

    fn __repr__(&self) -> String {
        format!("ArgLatCrossing(event={:?}, time={})", self.event, self.time)
    }
}
