use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};

/// Whether the satellite is sunlit or in Earth's shadow.
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
pub enum IlluminationState {
    /// The satellite is illuminated by the Sun.
    Sunlit,
    /// The satellite is in Earth's shadow (cylindrical umbra model).
    Eclipse,
}

/// A contiguous window of constant illumination state.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Illumination {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub state: IlluminationState,
}

#[gen_stub_pymethods]
#[pymethods]
impl Illumination {
    /// Start of the illumination window (UTC).
    #[getter]
    fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// End of the illumination window (UTC).
    #[getter]
    fn end(&self) -> DateTime<Utc> {
        self.end
    }

    /// Illumination state (Sunlit or Eclipse).
    #[getter]
    fn state(&self) -> IlluminationState {
        self.state
    }

    /// Duration of the window in seconds.
    #[getter]
    fn duration_seconds(&self) -> f64 {
        (self.end - self.start).num_milliseconds() as f64 / 1000.0
    }

    fn __repr__(&self) -> String {
        format!(
            "Illumination(state={:?}, start={}, end={})",
            self.state, self.start, self.end,
        )
    }
}
