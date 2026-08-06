use pyo3::PyErr;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use sgp4_predict::Error;

pub fn to_py_err(e: Error) -> PyErr {
    match e {
        Error::TleFormat(_)
        | Error::Tle(_)
        | Error::Elements(_)
        | Error::Interval(_)
        // A rejected area is bad input, not a runtime failure.
        | Error::Aoi(_) => PyValueError::new_err(e.to_string()),
        // Sgp4, Roots, Detect, Custom, and — since `Error` is
        // `#[non_exhaustive]` — any variant added later.
        _ => PyRuntimeError::new_err(e.to_string()),
    }
}
