use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use sgp4_predict::{HasId, HasTle, Observer};

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

/// A fixed point on Earth's surface from which satellite passes are observed.
///
/// Latitude and longitude are in degrees. Altitude is in metres above the WGS-84 ellipsoid.
#[gen_stub_pyclass]
#[pyclass(
    frozen,
    name = "Observer",
    from_py_object,
    module = "sgp4_predict._sgp4_predict"
)]
#[derive(Clone)]
pub struct GroundObserver {
    latitude_deg: f64,
    longitude_deg: f64,
    altitude: f64,
}

#[gen_stub_pymethods]
#[pymethods]
impl GroundObserver {
    #[new]
    pub fn new(latitude_deg: f64, longitude_deg: f64, altitude: f64) -> Self {
        Self {
            latitude_deg,
            longitude_deg,
            altitude,
        }
    }

    /// Geodetic latitude in degrees (positive north).
    #[getter]
    fn latitude_deg(&self) -> f64 {
        self.latitude_deg
    }

    /// Geodetic longitude in degrees (positive east).
    #[getter]
    fn longitude_deg(&self) -> f64 {
        self.longitude_deg
    }

    /// Height above the WGS-84 ellipsoid in metres.
    #[getter]
    fn altitude(&self) -> f64 {
        self.altitude
    }
}

impl Observer for GroundObserver {
    fn latitude_deg(&self) -> f64 {
        self.latitude_deg
    }
    fn longitude_deg(&self) -> f64 {
        self.longitude_deg
    }
    fn altitude(&self) -> f64 {
        self.altitude
    }
}
