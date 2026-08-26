use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

pub(crate) fn duration_seconds(start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
    (end - start).num_milliseconds() as f64 / 1000.0
}

/// Emits the whole `#[pymethods]` block for a `[start, end)` window type; pyo3
/// permits only one per class, so per-type members come in as `$extra`.
macro_rules! window_pymethods {
    (
        $ty:ident,
        start: $start_doc:literal,
        end: $end_doc:literal,
        duration_seconds: $duration_doc:literal,
        $($extra:tt)*
    ) => {
        $crate::types::window::window_pymethods!(@impl $ty, $start_doc, $end_doc, {
            #[doc = $duration_doc]
            #[doc = ""]
            #[doc = "Deprecated: use `duration`, which is a `timedelta`."]
            #[getter]
            fn duration_seconds(&self, py: ::pyo3::Python<'_>) -> ::pyo3::PyResult<f64> {
                ::pyo3::PyErr::warn(
                    py,
                    &py.get_type::<::pyo3::exceptions::PyDeprecationWarning>(),
                    c"duration_seconds is deprecated; use duration instead",
                    1,
                )?;
                Ok($crate::types::window::duration_seconds(self.start, self.end))
            }
        } $($extra)*);
    };

    (
        $ty:ident,
        start: $start_doc:literal,
        end: $end_doc:literal,
        $($extra:tt)*
    ) => {
        $crate::types::window::window_pymethods!(@impl $ty, $start_doc, $end_doc, {} $($extra)*);
    };

    (@impl $ty:ident, $start_doc:literal, $end_doc:literal, {$($deprecated:tt)*} $($extra:tt)*) => {
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

            $($deprecated)*
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

    #[new]
    fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    fn __repr__(&self) -> String {
        format!("Interval(start={}, end={})", self.start, self.end)
    }
}
