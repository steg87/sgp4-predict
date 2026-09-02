//! Argument newtypes that convert at the FFI boundary.
//!
//! A `&Bound<'_, PyAny>` parameter tells `pyo3-stub-gen` nothing, so it lands
//! in the stub as `typing.Any`. Each type here pairs the conversion
//! (`FromPyObject`) with the Python type annotation it should carry
//! (`PyStubType`), so a method signature states both once by naming the type
//! rather than repeating an `override_type` attribute per argument.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use pyo3::Borrowed;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3_stub_gen::{PyStubType, TypeInfo};
use sgp4_predict::Degrees;

use crate::area::{GeodeticPoint, LatLon};

/// A type alias defined in the hand-written `sgp4_predict/__init__.pyi`, which
/// the generated stub imports by module.
fn alias(name: &str) -> TypeInfo {
    TypeInfo {
        name: name.to_string(),
        source_module: None,
        import: HashSet::from(["sgp4_predict".into()]),
        type_refs: HashMap::new(),
    }
}

/// A point argument: a `LatLon`, a `GeodeticPoint` whose altitude is ignored, or a
/// `(latitude_deg, longitude_deg)` tuple.
pub(crate) struct LatLonArg(pub(crate) sgp4_predict::LatLon);

impl<'py> FromPyObject<'_, 'py> for LatLonArg {
    type Error = PyErr;

    fn extract(point: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        // `cast` rather than `extract`: a miss here is the common case for the
        // tuple form, and only `cast` misses without building an exception.
        if let Ok(p) = point.cast::<LatLon>() {
            return Ok(Self(p.get().inner));
        }
        if let Ok(p) = point.cast::<GeodeticPoint>() {
            return Ok(Self(p.get().inner.into()));
        }
        let (latitude_deg, longitude_deg) = point.extract::<(f64, f64)>().map_err(|_| {
            PyTypeError::new_err(
                "expected a LatLon, a GeodeticPoint, or a (latitude_deg, longitude_deg) tuple",
            )
        })?;
        Ok(Self(sgp4_predict::LatLon::new(
            Degrees(latitude_deg),
            Degrees(longitude_deg),
        )))
    }
}

impl PyStubType for LatLonArg {
    fn type_output() -> TypeInfo {
        alias("sgp4_predict.LatLonLike")
    }
}

/// A time-range argument: anything exposing `start` and `end` datetimes.
pub(crate) struct IntervalArg {
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: DateTime<Utc>,
}

impl<'py> FromPyObject<'_, 'py> for IntervalArg {
    type Error = PyErr;

    fn extract(interval: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        Ok(Self {
            start: interval.getattr("start")?.extract()?,
            end: interval.getattr("end")?.extract()?,
        })
    }
}

impl PyStubType for IntervalArg {
    fn type_output() -> TypeInfo {
        alias("sgp4_predict.IntervalRange")
    }
}
