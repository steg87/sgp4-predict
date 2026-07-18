use pyo3::PyErr;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use sgp4_predict::Error;

pub fn to_py_err(e: Error) -> PyErr {
    match e {
        Error::TleFormat(_) | Error::Tle(_) | Error::Elements(_) | Error::Interval(_) => {
            PyValueError::new_err(e.to_string())
        }
        Error::Sgp4(_)
        | Error::Roots(_)
        | Error::Transit(_)
        | Error::Detect(_)
        | Error::Custom(_) => PyRuntimeError::new_err(e.to_string()),
    }
}
