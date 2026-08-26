use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// Emits the whole `#[pymethods]` block for a `[start, end)` window type.
///
/// pyo3 permits one `#[pymethods]` impl per class, so anything a single type
/// adds — `Illumination::state`, `Interval::new`, every `__repr__` — is passed
/// in as the trailing token stream and spliced into the same block.
macro_rules! window_pymethods {
    (
        $ty:ident,
        start: $start_doc:literal,
        end: $end_doc:literal,
        duration_seconds: $duration_doc:literal,
        $($extra:tt)*
    ) => {
        #[::pyo3_stub_gen::derive::gen_stub_pymethods]
        #[::pyo3::pymethods]
        impl $ty {
            #[doc = $start_doc]
            #[getter]
            fn start(&self) -> ::chrono::DateTime<::chrono::Utc> {
                self.start
            }

            #[doc = $end_doc]
            #[getter]
            fn end(&self) -> ::chrono::DateTime<::chrono::Utc> {
                self.end
            }

            #[doc = $duration_doc]
            #[getter]
            fn duration_seconds(&self) -> f64 {
                (self.end - self.start).num_milliseconds() as f64 / 1000.0
            }

            /// Length of the interval.
            #[getter]
            fn duration(&self) -> ::chrono::TimeDelta {
                self.end - self.start
            }

            /// The instant halfway between `start` and `end`.
            #[getter]
            fn mid_point(&self) -> ::chrono::DateTime<::chrono::Utc> {
                self.start + (self.end - self.start) / 2
            }

            /// The overlap with `other`, or None if the two do not overlap.
            fn intersection(
                &self,
                other: $crate::convert::IntervalArg,
            ) -> Option<$crate::types::Interval> {
                let start = self.start.max(other.start);
                let end = self.end.min(other.end);
                (start < end).then(|| $crate::types::Interval { start, end })
            }

            $($extra)*
        }
    };
}
pub(crate) use window_pymethods;

/// A concrete `[start, end)` interval.
#[gen_stub_pyclass]
#[pyclass(eq, hash, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Interval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

window_pymethods! {
    Interval,
    start: "Inclusive start of the interval.",
    end: "Exclusive end of the interval.",
    duration_seconds: "Length of the interval in seconds.",

    #[new]
    fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    fn __repr__(&self) -> String {
        format!("Interval(start={}, end={})", self.start, self.end)
    }
}
