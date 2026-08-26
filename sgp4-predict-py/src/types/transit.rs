use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// A satellite pass — the window during which the satellite is above the minimum elevation.
#[gen_stub_pyclass]
#[pyclass(eq, hash, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Transit {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

crate::types::window::window_pymethods! {
    Transit,
    start: "Acquisition of Signal: when the satellite rises above `min_elevation`.",
    end: "Loss of Signal: when the satellite drops below `min_elevation`.",
    duration_seconds: "Duration of the transit in seconds.",

    fn __repr__(&self) -> String {
        format!(
            "Transit(start={}, end={}, duration={:.1}s)",
            self.start,
            self.end,
            crate::types::window::duration_seconds(self.start, self.end),
        )
    }
}
