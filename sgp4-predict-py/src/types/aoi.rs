use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// The window during which an area is within the payload's reach.
#[gen_stub_pyclass]
#[pyclass(eq, hash, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AoiWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

crate::types::window::window_pymethods! {
    AoiWindow,
    start: "When the area comes within reach.",
    end: "When it passes back out of reach.",
    duration_seconds: "Duration of the window in seconds.",

    fn __repr__(&self) -> String {
        format!(
            "AoiWindow(start={}, end={}, duration={:.1}s)",
            self.start,
            self.end,
            crate::types::window::duration_seconds(self.start, self.end),
        )
    }
}
