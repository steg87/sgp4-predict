use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use sgp4_predict::TleRecord;

use crate::elements::Elements;

/// A satellite TLE (Two-Line Element set) identified by a name and two TLE lines.
#[gen_stub_pyclass]
#[pyclass(eq, hash, frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Tle {
    satellite_name: String,
    line_1: String,
    line_2: String,
}

#[gen_stub_pymethods]
#[pymethods]
impl Tle {
    #[new]
    pub fn new(satellite_name: String, line_1: String, line_2: String) -> Self {
        Self {
            satellite_name,
            line_1,
            line_2,
        }
    }

    #[getter]
    fn satellite_name(&self) -> &str {
        &self.satellite_name
    }

    #[getter]
    fn line_1(&self) -> &str {
        &self.line_1
    }

    #[getter]
    fn line_2(&self) -> &str {
        &self.line_2
    }

    /// Parse this TLE into orbital elements.
    ///
    /// Raises `ValueError` if the TLE is malformed.
    fn to_elements(&self) -> PyResult<Elements> {
        sgp4_predict::Elements::from_tle(
            Some(self.satellite_name.clone()),
            self.line_1.as_bytes(),
            self.line_2.as_bytes(),
        )
        .map(|inner| Elements { inner })
        .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

impl TleRecord for Tle {
    fn satellite_name(&self) -> &str {
        &self.satellite_name
    }

    fn line_1(&self) -> &str {
        &self.line_1
    }

    fn line_2(&self) -> &str {
        &self.line_2
    }
}
