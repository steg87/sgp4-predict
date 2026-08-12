use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};
use sgp4_predict::{Area, Degrees, Radians};

use crate::{convert::LatLonArg, errors::to_py_err};

// ── LatLon ─────────────────────────────────────────────────────────────────────

/// A point on Earth's surface, in degrees.
///
/// Anywhere a `LatLon` is accepted, a plain `(latitude_deg, longitude_deg)`
/// tuple works too.
#[gen_stub_pyclass]
#[pyclass(eq, frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub(crate) inner: sgp4_predict::LatLon,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geodetic {
    pub(crate) inner: sgp4_predict::Geodetic,
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

// ── Coverage ───────────────────────────────────────────────────────────────────

/// Whether any part of an area, or all of it, must be within reach for a window
/// to be open.
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
pub enum Coverage {
    /// Any part of the area is within reach.
    Any,
    /// Every part of the area is within reach at once. Needs `max_off_nadir_deg`
    /// wider than the area — at the default of zero the reach is a single point,
    /// so no window opens. Not the same as "one image covers the area", which
    /// depends on the instantaneous field of view.
    Full,
}

impl From<Coverage> for sgp4_predict::Coverage {
    fn from(c: Coverage) -> Self {
        match c {
            Coverage::Any => Self::Any,
            Coverage::Full => Self::Full,
        }
    }
}

// ── Polygon ────────────────────────────────────────────────────────────────────

/// A closed polygon on Earth's surface whose edges are great-circle arcs.
///
/// The ring closes implicitly and vertex order does not matter. Vertices may be
/// `LatLon` objects, `Geodetic` objects whose altitude is ignored, or
/// `(latitude_deg, longitude_deg)` tuples.
///
/// Edges are great-circle arcs, so vertices at the same latitude are not joined
/// along the parallel — the arc bows toward the nearer pole, growing with the
/// square of the edge's longitude span. A 7° edge at 60°N bulges about 0.05°;
/// vertices a quarter of the globe apart reach roughly 68°N. Use `Rectangle` when
/// the region really is "these latitudes by these longitudes".
///
/// Raises `ValueError` if fewer than three distinct vertices remain, if a latitude
/// is outside [-90, 90], if a coordinate is `nan` or infinite, if consecutive
/// vertices are antipodal, or if the polygon does not fit inside a hemisphere.
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    inner: sgp4_predict::Polygon,
    // Kept alongside `inner`, which has no accessor for it. Only `new` writes
    // either, so the two cannot diverge.
    fill_rule: FillRule,
}

#[gen_stub_pymethods]
#[pymethods]
impl Polygon {
    #[new]
    #[pyo3(signature = (vertices, fill_rule = FillRule::NonZero))]
    fn new(
        #[gen_stub(override_type(type_repr = "collections.abc.Iterable[sgp4_predict.LatLonLike]", imports = ("collections.abc", "sgp4_predict")))]
        vertices: &Bound<'_, PyAny>,
        fill_rule: FillRule,
    ) -> PyResult<Self> {
        let mut points = Vec::new();
        for vertex in vertices.try_iter()? {
            points.push(vertex?.extract::<LatLonArg>()?.0);
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
    fn signed_angular_offset_deg(&self, point: LatLonArg) -> f64 {
        self.inner.signed_angular_offset(point.0).degrees()
    }

    /// Angular distance from a point to the farthest point of the area, in
    /// degrees. This is what `coverage="full"` is measured against.
    fn max_angular_distance_deg(&self, point: LatLonArg) -> f64 {
        self.inner.max_angular_distance(point.0).degrees()
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
/// objects, `Geodetic` objects whose altitude is ignored, or
/// `(latitude_deg, longitude_deg)` tuples.
///
/// Raises `ValueError` if a latitude is outside [-90, 90], if a coordinate is `nan`
/// or infinite, or if the box has no extent. Note that -180 and 180 are the same
/// meridian, so use `latitude_band` for a box spanning every longitude.
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, Clone, PartialEq)]
pub struct Rectangle {
    inner: sgp4_predict::Rectangle,
}

#[gen_stub_pymethods]
#[pymethods]
impl Rectangle {
    #[new]
    fn new(south_west: LatLonArg, north_east: LatLonArg) -> PyResult<Self> {
        let inner = sgp4_predict::Rectangle::new(south_west.0, north_east.0).map_err(to_py_err)?;
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
    fn signed_angular_offset_deg(&self, point: LatLonArg) -> f64 {
        self.inner.signed_angular_offset(point.0).degrees()
    }

    /// Angular distance from a point to the farthest point of the area, in
    /// degrees. This is what `coverage="full"` is measured against.
    fn max_angular_distance_deg(&self, point: LatLonArg) -> f64 {
        self.inner.max_angular_distance(point.0).degrees()
    }

    fn __repr__(&self) -> String {
        let (south, north) = self.latitudes_deg();
        let (west, span) = self.longitudes_deg();
        format!("Rectangle(latitudes=({south}, {north}), west={west}, span={span})")
    }
}

// ── Circle ─────────────────────────────────────────────────────────────────────

/// A circular area on Earth's surface — a spherical cap.
///
/// The radius is angular: a degree of arc is about 111.2 km on the ground. The
/// centre may be a `LatLon` object, a `Geodetic` object whose altitude is ignored,
/// or a `(latitude_deg, longitude_deg)` tuple.
///
/// Raises `ValueError` if the radius is outside `(0, 90)`, if the centre's latitude
/// is outside [-90, 90], or if any argument is `nan` or infinite.
#[gen_stub_pyclass]
#[pyclass(frozen, from_py_object, module = "sgp4_predict._sgp4_predict")]
#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    inner: sgp4_predict::Circle,
}

#[gen_stub_pymethods]
#[pymethods]
impl Circle {
    #[new]
    fn new(centre: LatLonArg, radius_deg: f64) -> PyResult<Self> {
        let inner = sgp4_predict::Circle::new(centre.0, Degrees(radius_deg)).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    /// The circle's centre.
    #[getter]
    fn centre(&self) -> LatLon {
        LatLon::from_inner(self.inner.centre())
    }

    /// The circle's angular radius, in degrees of arc.
    #[getter]
    fn radius_deg(&self) -> f64 {
        self.inner.radius().to_f64()
    }

    /// Signed angular offset of a point from the boundary, in degrees:
    /// positive inside, negative outside, zero on the boundary.
    fn signed_angular_offset_deg(&self, point: LatLonArg) -> f64 {
        self.inner.signed_angular_offset(point.0).degrees()
    }

    /// Angular distance from a point to the farthest point of the area, in
    /// degrees. This is what `coverage="full"` is measured against.
    fn max_angular_distance_deg(&self, point: LatLonArg) -> f64 {
        self.inner.max_angular_distance(point.0).degrees()
    }

    fn __repr__(&self) -> String {
        let centre = self.centre();
        format!(
            "Circle(centre=({}, {}), radius_deg={})",
            centre.latitude_deg(),
            centre.longitude_deg(),
            self.radius_deg()
        )
    }
}

// ── dispatch ───────────────────────────────────────────────────────────────────

/// The library's `AoiIter` is generic over one `Area`, so the Python side needs a
/// single concrete type covering all three shapes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AreaKind {
    Polygon(sgp4_predict::Polygon),
    Rectangle(sgp4_predict::Rectangle),
    Circle(sgp4_predict::Circle),
}

/// The borrowed counterpart, for the one-shot paths that don't outlive the
/// argument and so need no clone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AreaRef<'a> {
    Polygon(&'a sgp4_predict::Polygon),
    Rectangle(&'a sgp4_predict::Rectangle),
    Circle(&'a sgp4_predict::Circle),
}

impl Area for AreaKind {
    fn signed_angular_offset(&self, point: sgp4_predict::LatLon) -> Radians {
        match self {
            Self::Polygon(a) => a.signed_angular_offset(point),
            Self::Rectangle(a) => a.signed_angular_offset(point),
            Self::Circle(a) => a.signed_angular_offset(point),
        }
    }
}

impl Area for AreaRef<'_> {
    fn signed_angular_offset(&self, point: sgp4_predict::LatLon) -> Radians {
        match self {
            Self::Polygon(a) => a.signed_angular_offset(point),
            Self::Rectangle(a) => a.signed_angular_offset(point),
            Self::Circle(a) => a.signed_angular_offset(point),
        }
    }
}

pub(crate) fn extract_area(area: &Bound<'_, PyAny>) -> PyResult<AreaKind> {
    Ok(match extract_area_ref(area)? {
        AreaRef::Polygon(a) => AreaKind::Polygon(a.clone()),
        AreaRef::Rectangle(a) => AreaKind::Rectangle(a.clone()),
        AreaRef::Circle(a) => AreaKind::Circle(a.clone()),
    })
}

// All three shapes are `frozen`, so `get()` hands back a reference without a
// clone — unlike `extract`, which would copy the whole vertex list.
pub(crate) fn extract_area_ref<'a>(area: &'a Bound<'_, PyAny>) -> PyResult<AreaRef<'a>> {
    if let Ok(a) = area.cast::<Polygon>() {
        return Ok(AreaRef::Polygon(&a.get().inner));
    }
    if let Ok(a) = area.cast::<Rectangle>() {
        return Ok(AreaRef::Rectangle(&a.get().inner));
    }
    if let Ok(a) = area.cast::<Circle>() {
        return Ok(AreaRef::Circle(&a.get().inner));
    }
    Err(PyTypeError::new_err(
        "expected a Polygon, Rectangle, or Circle",
    ))
}
