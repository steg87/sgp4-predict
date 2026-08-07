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
        Error::Sgp4(_) | Error::Roots(_) | Error::Detect(_) | Error::Custom(_) => {
            PyRuntimeError::new_err(e.to_string())
        }
        // `Error` is `#[non_exhaustive]`; an unknown future variant is likelier
        // a runtime failure than bad input.
        _ => PyRuntimeError::new_err(e.to_string()),
    }
}
