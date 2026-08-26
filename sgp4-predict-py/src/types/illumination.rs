use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum};

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
#[pyclass(eq, hash, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Illumination {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub state: IlluminationState,
}

crate::types::window::window_pymethods! {
    Illumination,
    start: "Start of the illumination window (UTC).",
    end: "End of the illumination window (UTC).",
    duration_seconds: "Duration of the window in seconds.",

    /// Illumination state (Sunlit or Eclipse).
    #[getter]
    fn state(&self) -> IlluminationState {
        self.state
    }

    fn __repr__(&self) -> String {
        format!(
            "Illumination(state={:?}, start={}, end={})",
            self.state, self.start, self.end,
        )
    }
}
