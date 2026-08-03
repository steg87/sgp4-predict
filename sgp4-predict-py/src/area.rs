use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};
use sgp4_predict::{Area, Degrees, Radians};

use crate::errors::to_py_err;

// ── LatLon ─────────────────────────────────────────────────────────────────────

/// A point on Earth's surface, in degrees.
///
/// Anywhere a `LatLon` is accepted, a plain `(latitude_deg, longitude_deg)`
/// tuple works too.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Clone, Copy, PartialEq)]
pub struct LatLon {
    inner: sgp4_predict::LatLon,
}

#[gen_stub_pymethods]
#[pymethods]
impl LatLon {
    #[new]
    fn new(latitude_deg: f64, longitude_deg: f64) -> Self {
        Self {
            inner: sgp4_predict::LatLon::new(Degrees(latitude_deg), Degrees(longitude_deg)),
        }
    }

    /// Geodetic latitude in degrees (positive north).
    #[getter]
    fn latitude_deg(&self) -> f64 {
        self.inner.latitude.to_f64()
    }

    /// Geodetic longitude in degrees (positive east).
    #[getter]
    fn longitude_deg(&self) -> f64 {
        self.inner.longitude.to_f64()
    }

    fn __repr__(&self) -> String {
        format!(
            "LatLon(latitude_deg={}, longitude_deg={})",
            self.latitude_deg(),
            self.longitude_deg()
        )
    }
}

impl LatLon {
    fn from_inner(inner: sgp4_predict::LatLon) -> Self {
        Self { inner }
    }
}

// ── Geodetic ───────────────────────────────────────────────────────────────────

/// A geodetic position on or above the WGS-84 ellipsoid.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Clone, Copy, PartialEq)]
pub struct Geodetic {
    inner: sgp4_predict::Geodetic,
}

#[gen_stub_pymethods]
#[pymethods]
impl Geodetic {
    #[new]
    fn new(latitude_deg: f64, longitude_deg: f64, altitude: f64) -> Self {
        Self {
            inner: sgp4_predict::Geodetic {
                latitude: Degrees(latitude_deg),
                longitude: Degrees(longitude_deg),
                altitude,
            },
        }
    }

    /// Geodetic latitude in degrees (positive north).
    #[getter]
    fn latitude_deg(&self) -> f64 {
        self.inner.latitude.to_f64()
    }

    /// Geodetic longitude in degrees (positive east).
    #[getter]
    fn longitude_deg(&self) -> f64 {
        self.inner.longitude.to_f64()
    }

    /// Height above the WGS-84 ellipsoid in metres.
    #[getter]
    fn altitude(&self) -> f64 {
        self.inner.altitude
    }

    fn __repr__(&self) -> String {
        format!(
            "Geodetic(latitude_deg={}, longitude_deg={}, altitude={})",
            self.latitude_deg(),
            self.longitude_deg(),
            self.altitude()
        )
    }
}

impl Geodetic {
    pub(crate) fn from_inner(inner: sgp4_predict::Geodetic) -> Self {
        Self { inner }
    }
}

// ── FillRule ───────────────────────────────────────────────────────────────────

/// How the interior of a self-intersecting `Polygon` is determined.
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
pub enum FillRule {
    /// Inside wherever the winding number is non-zero. A ring that crosses itself stays filled.
    NonZero,
    /// Inside wherever the winding number is odd. A ring that doubles back leaves a hole.
    EvenOdd,
}

impl From<FillRule> for sgp4_predict::FillRule {
    fn from(f: FillRule) -> Self {
        match f {
            FillRule::NonZero => Self::NonZero,
            FillRule::EvenOdd => Self::EvenOdd,
        }
    }
}

// ── Polygon ────────────────────────────────────────────────────────────────────

/// A closed polygon on Earth's surface whose edges are great-circle arcs.
///
/// The ring closes implicitly and vertex order does not matter. Vertices may be
/// `LatLon` objects or `(latitude_deg, longitude_deg)` tuples.
///
/// Edges are great-circle arcs, so they are not lines of constant latitude — four
/// vertices at 60°N bulge to roughly 68°N between them. Use `Rectangle` when the
/// region really is "these latitudes by these longitudes".
///
/// Raises `ValueError` if fewer than three distinct vertices remain, if a latitude
/// is outside [-90, 90], if consecutive vertices are antipodal, or if the polygon
/// does not fit inside a hemisphere.
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Clone)]
pub struct Polygon {
    inner: sgp4_predict::Polygon,
    fill_rule: FillRule,
}

#[gen_stub_pymethods]
#[pymethods]
impl Polygon {
    #[new]
    #[pyo3(signature = (vertices, fill_rule = FillRule::NonZero))]
    fn new(vertices: &Bound<'_, PyAny>, fill_rule: FillRule) -> PyResult<Self> {
        let mut points = Vec::new();
        for vertex in vertices.try_iter()? {
            points.push(extract_lat_lon(&vertex?)?);
        }
        let inner = sgp4_predict::Polygon::new(points)
            .map_err(to_py_err)?
            .with_fill_rule(fill_rule.into());
        Ok(Self { inner, fill_rule })
    }

    /// The polygon's vertices in ring order, after deduplication.
    #[getter]
    fn vertices(&self) -> Vec<LatLon> {
        self.inner.vertices().map(LatLon::from_inner).collect()
    }

    /// How the interior of a self-intersecting ring is determined.
    #[getter]
    fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    /// Signed angular offset of a point from the boundary, in degrees:
    /// positive inside, negative outside, zero on the boundary.
    fn signed_angular_offset_deg(&self, point: &Bound<'_, PyAny>) -> PyResult<f64> {
        offset_deg(&self.inner, point)
    }

    fn __repr__(&self) -> String {
        format!(
            "Polygon({} vertices, fill_rule={:?})",
            self.inner.vertices().len(),
            self.fill_rule
        )
    }
}

// ── Rectangle ──────────────────────────────────────────────────────────────────

/// A latitude/longitude box, whose north and south edges follow their parallels
/// exactly.
///
/// The box runs **eastward** from the south-west corner, so a north-east corner at
/// a smaller longitude wraps across the antimeridian. Corners may be `LatLon`
/// objects or `(latitude_deg, longitude_deg)` tuples.
///
/// Raises `ValueError` if a latitude is outside [-90, 90] or the box has no extent.
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Clone)]
pub struct Rectangle {
    inner: sgp4_predict::Rectangle,
}

#[gen_stub_pymethods]
#[pymethods]
impl Rectangle {
    #[new]
    fn new(south_west: &Bound<'_, PyAny>, north_east: &Bound<'_, PyAny>) -> PyResult<Self> {
        let inner = sgp4_predict::Rectangle::new(
            extract_lat_lon(south_west)?,
            extract_lat_lon(north_east)?,
        )
        .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// A box spanning every longitude between two latitudes — a band, or a polar
    /// cap when one latitude is a pole.
    #[staticmethod]
    fn latitude_band(south_deg: f64, north_deg: f64) -> PyResult<Self> {
        let inner = sgp4_predict::Rectangle::latitude_band(Degrees(south_deg), Degrees(north_deg))
            .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// The southern and northern latitude bounds, in degrees.
    #[getter]
    fn latitudes_deg(&self) -> (f64, f64) {
        let (south, north) = self.inner.latitudes();
        (south.to_f64(), north.to_f64())
    }

    /// The western bound and the extent eastward from it, in degrees. The extent is
    /// 360 for a box built by `latitude_band`.
    #[getter]
    fn longitudes_deg(&self) -> (f64, f64) {
        let (west, span) = self.inner.longitudes();
        (west.to_f64(), span.to_f64())
    }

    /// Signed angular offset of a point from the boundary, in degrees:
    /// positive inside, negative outside, zero on the boundary.
    fn signed_angular_offset_deg(&self, point: &Bound<'_, PyAny>) -> PyResult<f64> {
        offset_deg(&self.inner, point)
    }

    fn __repr__(&self) -> String {
        let (south, north) = self.latitudes_deg();
        let (west, span) = self.longitudes_deg();
        format!("Rectangle(latitudes=({south}, {north}), west={west}, span={span})")
    }
}

// ── Ellipse ────────────────────────────────────────────────────────────────────

/// An ellipse on Earth's surface: the points whose great-circle distances to two
/// foci sum to at most twice the semi-major axis.
///
/// Semi-axes are angular — a degree of arc is about 111.2 km on the ground — and
/// the bearing turns the major axis clockwise from north. At a pole, where north is
/// undefined, the bearing is measured from the prime meridian instead.
///
/// Raises `ValueError` unless `0 < semi_minor_deg <= semi_major_deg < 90`, or if the
/// centre's latitude is outside [-90, 90].
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Clone)]
pub struct Ellipse {
    inner: sgp4_predict::Ellipse,
}

#[gen_stub_pymethods]
#[pymethods]
impl Ellipse {
    #[new]
    #[pyo3(signature = (centre, semi_major_deg, semi_minor_deg, bearing_deg = 0.0))]
    fn new(
        centre: &Bound<'_, PyAny>,
        semi_major_deg: f64,
        semi_minor_deg: f64,
        bearing_deg: f64,
    ) -> PyResult<Self> {
        let inner = sgp4_predict::Ellipse::new(
            extract_lat_lon(centre)?,
            Degrees(semi_major_deg),
            Degrees(semi_minor_deg),
            Degrees(bearing_deg),
        )
        .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// A circular area of angular `radius_deg` — a spherical cap.
    #[staticmethod]
    fn circle(centre: &Bound<'_, PyAny>, radius_deg: f64) -> PyResult<Self> {
        let inner = sgp4_predict::Ellipse::circle(extract_lat_lon(centre)?, Degrees(radius_deg))
            .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// The ellipse's centre.
    #[getter]
    fn centre(&self) -> LatLon {
        LatLon::from_inner(self.inner.centre())
    }

    /// The semi-major axis, in degrees of arc.
    #[getter]
    fn semi_major_deg(&self) -> f64 {
        self.inner.semi_axes().0.to_f64()
    }

    /// The semi-minor axis, in degrees of arc.
    #[getter]
    fn semi_minor_deg(&self) -> f64 {
        self.inner.semi_axes().1.to_f64()
    }

    /// Bearing of the major axis, degrees clockwise from north.
    #[getter]
    fn bearing_deg(&self) -> f64 {
        self.inner.bearing().to_f64()
    }

    /// The two foci, which coincide with the centre when the ellipse is a circle.
    #[getter]
    fn foci(&self) -> (LatLon, LatLon) {
        let (a, b) = self.inner.foci();
        (LatLon::from_inner(a), LatLon::from_inner(b))
    }

    /// Signed angular offset of a point from the boundary, in degrees:
    /// positive inside, negative outside, zero on the boundary.
    fn signed_angular_offset_deg(&self, point: &Bound<'_, PyAny>) -> PyResult<f64> {
        offset_deg(&self.inner, point)
    }

    fn __repr__(&self) -> String {
        let centre = self.centre();
        format!(
            "Ellipse(centre=({}, {}), semi_major_deg={}, semi_minor_deg={}, bearing_deg={})",
            centre.latitude_deg(),
            centre.longitude_deg(),
            self.semi_major_deg(),
            self.semi_minor_deg(),
            self.bearing_deg()
        )
    }
}

// ── dispatch ───────────────────────────────────────────────────────────────────

/// The library's `AoiIter` is generic over one `Area`, so the Python side needs a
/// single concrete type covering all three shapes.
#[derive(Clone)]
pub(crate) enum AreaKind {
    Polygon(sgp4_predict::Polygon),
    Rectangle(sgp4_predict::Rectangle),
    Ellipse(sgp4_predict::Ellipse),
}

impl Area for AreaKind {
    fn signed_angular_offset(&self, point: sgp4_predict::LatLon) -> Radians {
        match self {
            Self::Polygon(a) => a.signed_angular_offset(point),
            Self::Rectangle(a) => a.signed_angular_offset(point),
            Self::Ellipse(a) => a.signed_angular_offset(point),
        }
    }
}

pub(crate) fn extract_area(area: &Bound<'_, PyAny>) -> PyResult<AreaKind> {
    if let Ok(a) = area.extract::<Polygon>() {
        return Ok(AreaKind::Polygon(a.inner));
    }
    if let Ok(a) = area.extract::<Rectangle>() {
        return Ok(AreaKind::Rectangle(a.inner));
    }
    if let Ok(a) = area.extract::<Ellipse>() {
        return Ok(AreaKind::Ellipse(a.inner));
    }
    Err(PyTypeError::new_err(
        "expected a Polygon, Rectangle, or Ellipse",
    ))
}

/// A `LatLon`, a `Geodetic`, or a `(latitude_deg, longitude_deg)` tuple.
pub(crate) fn extract_lat_lon(point: &Bound<'_, PyAny>) -> PyResult<sgp4_predict::LatLon> {
    if let Ok(p) = point.extract::<LatLon>() {
        return Ok(p.inner);
    }
    if let Ok(p) = point.extract::<Geodetic>() {
        return Ok(p.inner.into());
    }
    let (latitude_deg, longitude_deg) = point.extract::<(f64, f64)>().map_err(|_| {
        PyTypeError::new_err(
            "expected a LatLon, a Geodetic, or a (latitude_deg, longitude_deg) tuple",
        )
    })?;
    Ok(sgp4_predict::LatLon::new(
        Degrees(latitude_deg),
        Degrees(longitude_deg),
    ))
}

fn offset_deg(area: &impl Area, point: &Bound<'_, PyAny>) -> PyResult<f64> {
    Ok(area
        .signed_angular_offset(extract_lat_lon(point)?)
        .degrees())
}
