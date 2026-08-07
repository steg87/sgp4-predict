use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// The window during which the satellite's ground track lies inside an area.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AoiWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[gen_stub_pymethods]
#[pymethods]
impl AoiWindow {
    /// When the ground track crosses into the area.
    #[getter]
    fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// When it crosses back out.
    #[getter]
    fn end(&self) -> DateTime<Utc> {
        self.end
    }

    /// Duration of the window in seconds.
    #[getter]
    fn duration_seconds(&self) -> f64 {
        (self.end - self.start).num_milliseconds() as f64 / 1000.0
    }

    fn __repr__(&self) -> String {
        format!(
            "AoiWindow(start={}, end={}, duration={:.1}s)",
            self.start,
            self.end,
            self.duration_seconds(),
        )
    }
}
