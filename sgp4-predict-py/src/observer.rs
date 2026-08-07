use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use sgp4_predict::{Degrees, Observer};

/// A fixed point on Earth's surface from which satellite passes are observed.
///
/// Latitude and longitude are in degrees. Altitude is in metres above the WGS-84 ellipsoid.
#[gen_stub_pyclass]
#[pyclass(
    frozen,
    name = "GroundObserver",
    from_py_object,
    module = "sgp4_predict._sgp4_predict"
)]
#[derive(Debug, Clone, PartialEq)]
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
    fn latitude(&self) -> Degrees {
        Degrees(self.latitude_deg)
    }
    fn longitude(&self) -> Degrees {
        Degrees(self.longitude_deg)
    }
    fn altitude(&self) -> f64 {
        self.altitude
    }
}
