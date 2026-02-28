use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// A satellite pass — the window during which the satellite is above the minimum elevation.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
pub struct Transit {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[gen_stub_pymethods]
#[pymethods]
impl Transit {
    /// Acquisition of Signal: when the satellite rises above `min_elevation`.
    #[getter]
    fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Loss of Signal: when the satellite drops below `min_elevation`.
    #[getter]
    fn end(&self) -> DateTime<Utc> {
        self.end
    }

    /// Duration of the transit in seconds.
    #[getter]
    fn duration_seconds(&self) -> f64 {
        (self.end - self.start).num_milliseconds() as f64 / 1000.0
    }

    fn __repr__(&self) -> String {
        format!(
            "Transit(start={}, end={}, duration={:.1}s)",
            self.start,
            self.end,
            self.duration_seconds(),
        )
    }
}
