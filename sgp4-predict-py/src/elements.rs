use chrono::{DateTime, Utc};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

// ── Classification ─────────────────────────────────────────────────────────────

/// Satellite classification type (CLASSIFICATION_TYPE field in OMM).
#[gen_stub_pyclass_enum]
#[pyclass(
    eq,
    eq_int,
    hash,
    frozen,
    from_py_object,
    module = "sgp4_predict._sgp4_predict"
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Classification {
    /// Unclassified (U)
    Unclassified = 0,
    /// Classified (C)
    Classified = 1,
    /// Secret (S)
    Secret = 2,
}

impl From<Classification> for sgp4_predict::Classification {
    fn from(c: Classification) -> Self {
        match c {
            Classification::Unclassified => sgp4_predict::Classification::Unclassified,
            Classification::Classified => sgp4_predict::Classification::Classified,
            Classification::Secret => sgp4_predict::Classification::Secret,
        }
    }
}

impl From<sgp4_predict::Classification> for Classification {
    fn from(c: sgp4_predict::Classification) -> Self {
        match c {
            sgp4_predict::Classification::Unclassified => Classification::Unclassified,
            sgp4_predict::Classification::Classified => Classification::Classified,
            sgp4_predict::Classification::Secret => Classification::Secret,
        }
    }
}

// ── Elements ───────────────────────────────────────────────────────────────────

/// Orbital elements for a satellite.
///
/// Holds the data needed to initialise an SGP4 propagator.  Can be constructed
/// field-by-field or parsed directly from an OMM JSON string.
///
/// Example — parsing from a Space-Track / Celestrak API response:
///
/// ```python
/// import requests
/// from sgp4_predict import Elements, Predictor
///
/// data = requests.get("https://celestrak.org/SOCRATES/...").json()
/// elements = Elements.from_dict(data[0])
/// predictor = Predictor(elements)
/// ```
///
/// Example — manual construction:
///
/// ```python
/// from datetime import datetime, timezone
/// from sgp4_predict import Classification, Elements, Predictor
///
/// elements = Elements(
///     norad_id=25544,
///     epoch=datetime(2020, 7, 12, 1, 19, 7, tzinfo=timezone.utc),
///     mean_motion=15.49560532,
///     eccentricity=0.0001771,
///     inclination=51.6435,
///     right_ascension=225.4004,
///     argument_of_perigee=44.9625,
///     mean_anomaly=5.1087,
///     mean_motion_dot=0.00289036,
///     drag_term=0.0049645,
///     revolution_number=23587,
///     object_name="ISS (ZARYA)",
/// )
/// predictor = Predictor(elements)
/// ```
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug)]
pub struct Elements {
    pub(crate) inner: sgp4_predict::Elements,
}

#[gen_stub_pymethods]
#[pymethods]
impl Elements {
    /// Construct orbital elements from individual fields.
    ///
    /// `epoch` must be a timezone-aware `datetime` (UTC).
    /// All angle fields (`inclination`, `right_ascension`, `argument_of_perigee`,
    /// `mean_anomaly`) are in **degrees**.
    /// `mean_motion` is in **rev/day**.
    #[new]
    #[pyo3(signature = (
        *,
        norad_id,
        epoch,
        mean_motion,
        eccentricity,
        inclination,
        right_ascension,
        argument_of_perigee,
        mean_anomaly,
        mean_motion_dot,
        mean_motion_ddot = 0.0,
        drag_term = 0.0,
        revolution_number = 0,
        object_name = None,
        classification = Classification::Unclassified,
        international_designator = None,
        element_set_number = 0,
        ephemeris_type = 0,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        norad_id: u64,
        epoch: DateTime<Utc>,
        mean_motion: f64,
        eccentricity: f64,
        inclination: f64,
        right_ascension: f64,
        argument_of_perigee: f64,
        mean_anomaly: f64,
        mean_motion_dot: f64,
        mean_motion_ddot: f64,
        drag_term: f64,
        revolution_number: u64,
        object_name: Option<String>,
        classification: Classification,
        international_designator: Option<String>,
        element_set_number: u64,
        ephemeris_type: u8,
    ) -> Self {
        Self {
            inner: sgp4_predict::Elements {
                object_name,
                norad_id,
                classification: classification.into(),
                international_designator,
                datetime: epoch.naive_utc(),
                mean_motion_dot,
                mean_motion_ddot,
                drag_term,
                element_set_number,
                inclination,
                right_ascension,
                eccentricity,
                argument_of_perigee,
                mean_anomaly,
                mean_motion,
                revolution_number,
                ephemeris_type,
            },
        }
    }

    /// Parse an OMM JSON string into orbital elements.
    ///
    /// The JSON must use CCSDS OMM field names (`NORAD_CAT_ID`, `EPOCH`,
    /// `MEAN_MOTION`, `ECCENTRICITY`, etc.).  Both Celestrak and Space-Track
    /// JSON responses are supported.
    ///
    /// Raises `ValueError` if the JSON is malformed or missing required fields.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let inner: sgp4_predict::Elements =
            serde_json::from_str(json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Parse an OMM dict into orbital elements.
    ///
    /// The dict must use CCSDS OMM field names (`NORAD_CAT_ID`, `EPOCH`,
    /// `MEAN_MOTION`, `ECCENTRICITY`, etc.).  Both Celestrak and Space-Track
    /// JSON responses are supported — pass the dict directly without serialising
    /// to a string first.
    ///
    /// Raises `ValueError` if the dict is missing required fields or has invalid values.
    #[staticmethod]
    fn from_dict(
        #[gen_stub(override_type(type_repr = "builtins.dict", imports = ("builtins")))]
        data: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let json_module = data.py().import("json")?;
        let json_str: String = json_module.call_method1("dumps", (data,))?.extract()?;
        let inner: sgp4_predict::Elements =
            serde_json::from_str(&json_str).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Satellite name (OBJECT_NAME), if present.
    #[getter]
    fn object_name(&self) -> Option<&str> {
        self.inner.object_name.as_deref()
    }

    /// NORAD catalog number (NORAD_CAT_ID).
    #[getter]
    fn norad_id(&self) -> u64 {
        self.inner.norad_id
    }

    /// Classification type (CLASSIFICATION_TYPE).
    #[getter]
    fn classification(&self) -> Classification {
        self.inner.classification.clone().into()
    }

    /// International designator (OBJECT_ID), if present.
    #[getter]
    fn international_designator(&self) -> Option<&str> {
        self.inner.international_designator.as_deref()
    }

    /// Element set epoch in UTC (EPOCH).
    #[getter]
    fn epoch(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.inner.datetime, Utc)
    }

    /// First derivative of mean motion / 2 (MEAN_MOTION_DOT), rev/day².
    #[getter]
    fn mean_motion_dot(&self) -> f64 {
        self.inner.mean_motion_dot
    }

    /// Second derivative of mean motion / 6 (MEAN_MOTION_DDOT), rev/day³.
    #[getter]
    fn mean_motion_ddot(&self) -> f64 {
        self.inner.mean_motion_ddot
    }

    /// BSTAR drag term (BSTAR), 1/earth-radii.
    #[getter]
    fn drag_term(&self) -> f64 {
        self.inner.drag_term
    }

    /// Element set number (ELEMENT_SET_NO).
    #[getter]
    fn element_set_number(&self) -> u64 {
        self.inner.element_set_number
    }

    /// Inclination (INCLINATION), degrees.
    #[getter]
    fn inclination(&self) -> f64 {
        self.inner.inclination
    }

    /// Right ascension of ascending node (RA_OF_ASC_NODE), degrees.
    #[getter]
    fn right_ascension(&self) -> f64 {
        self.inner.right_ascension
    }

    /// Eccentricity (ECCENTRICITY).
    #[getter]
    fn eccentricity(&self) -> f64 {
        self.inner.eccentricity
    }

    /// Argument of perigee (ARG_OF_PERICENTER), degrees.
    #[getter]
    fn argument_of_perigee(&self) -> f64 {
        self.inner.argument_of_perigee
    }

    /// Mean anomaly (MEAN_ANOMALY), degrees.
    #[getter]
    fn mean_anomaly(&self) -> f64 {
        self.inner.mean_anomaly
    }

    /// Mean motion (MEAN_MOTION), rev/day.
    #[getter]
    fn mean_motion(&self) -> f64 {
        self.inner.mean_motion
    }

    /// Revolution number at epoch (REV_AT_EPOCH).
    #[getter]
    fn revolution_number(&self) -> u64 {
        self.inner.revolution_number
    }

    /// Ephemeris type (EPHEMERIS_TYPE). Normally 0.
    #[getter]
    fn ephemeris_type(&self) -> u8 {
        self.inner.ephemeris_type
    }

    fn __repr__(&self) -> String {
        let name_repr = match &self.inner.object_name {
            Some(name) => format!("{name:?}"),
            None => "None".to_string(),
        };
        format!(
            "Elements(norad_id={}, object_name={}, epoch={})",
            self.inner.norad_id,
            name_repr,
            DateTime::<Utc>::from_naive_utc_and_offset(self.inner.datetime, Utc),
        )
    }
}
