use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use sgp4_predict::{HasId, HasTle};

/// A satellite TLE (Two-Line Element set) identified by a name and two TLE lines.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
pub struct Tle {
    id: String,
    line_1: String,
    line_2: String,
}

#[gen_stub_pymethods]
#[pymethods]
impl Tle {
    #[new]
    pub fn new(id: String, line_1: String, line_2: String) -> Self {
        Self { id, line_1, line_2 }
    }

    #[getter]
    fn id(&self) -> &str {
        &self.id
    }

    #[getter]
    fn line_1(&self) -> &str {
        &self.line_1
    }

    #[getter]
    fn line_2(&self) -> &str {
        &self.line_2
    }
}

impl HasId for Tle {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasTle for Tle {
    fn line_1(&self) -> &str {
        &self.line_1
    }
    fn line_2(&self) -> &str {
        &self.line_2
    }
}
