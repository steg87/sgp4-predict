//! Area-of-interest detection: when can the satellite reach a region on
//! Earth's surface?
//!
//! [`AoiIter`] yields the [`AoiWindow`]s during which an [`Area`] lies within
//! the payload's reach. It is a thin wrapper over the generic
//! [`WindowIter`](crate::WindowIter), like [`TransitIter`](crate::TransitIter):
//! the event function is a signed angular offset from the area's boundary, and
//! the windows are where it is positive.
//!
//! Reach is set by [`AoiIterOpts::max_off_nadir`], the half-angle of the
//! satellite's field of regard. It defaults to zero, which detects the ground
//! track itself crossing into the area. [`AoiIterOpts::coverage`] chooses
//! whether any part of the area or all of it must be within reach.
//!
//! [`Polygon`] is the general shape — an arbitrary ring of latitude/longitude
//! vertices, which may be concave or self-intersecting. [`Rectangle`] is a
//! plain latitude/longitude box, and [`Circle`] a spherical cap. Implement
//! [`Area`] on your own type for shapes this crate does not provide.
//!
//! An [`AoiWindow`] implements [`IntervalRange`], so it can be passed directly
//! to [`Predictor::prediction_iter`] or [`Predictor::observation_iter`] to
//! iterate over a specific overpass.
//!
//! # Geometry
//!
//! [`Polygon`] edges are **great-circle arcs**, in the sphere obtained by
//! treating geodetic latitude as spherical latitude — the same convention as
//! S2 and BigQuery GIS. They are neither rhumb lines nor lines of constant
//! latitude: an edge joining two vertices at the same latitude bows toward the
//! nearer pole, by an amount growing with the square of its longitude span. At
//! 60°N a 5° edge bulges 0.02° (under 3 km) and a 10° edge 0.09°, while
//! vertices a quarter of the globe apart reach roughly 68°N.
//!
//! The bow is always toward the *nearer* pole, so both horizontal edges of a
//! "box" shift the same way. The region is displaced poleward rather than
//! simply enlarged: it takes in ground beyond the far edge and gives up ground
//! just inside the near one. Either densify the long edges — a box a few
//! degrees wide needs nothing — or use [`Rectangle`], whose north and south
//! edges follow their parallels exactly.
//!
//! Note that this differs from GeoJSON: RFC 7946 §3.1.1 defines an edge as a
//! straight line in longitude/latitude. That convention cannot represent a
//! polygon containing a pole, and needs explicit unwrapping at the
//! antimeridian; great-circle edges need neither.
//!
//! [`IntervalRange`]: crate::IntervalRange
//! [`Predictor::prediction_iter`]: crate::Predictor::prediction_iter
//! [`Predictor::observation_iter`]: crate::Predictor::observation_iter

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use sgp4::Elements;
use thiserror::Error as ThisError;

use crate::{
    Predictor, Result,
    angle::{Degrees, Radians},
    detect::{self, EventFunction, Sample, StepStrategy, WindowIter},
    frames::{LatLon, WGS84_E2},
    roots::Refinement,
    time::{self, IntervalRange},
};

/// Vertices closer together than this are treated as duplicates.
const COINCIDENT: f64 = 1e-9;

/// A ground point within this angle of the boundary is reported as exactly on
/// it. Roughly 6 nanometres of arc.
const ON_BOUNDARY: f64 = 1e-15;

/// Floor on the adaptive coarse-scan step, deliberately finer than
/// `detect::MIN_POSITIVE_STEP`: `min_step` bounds the shortest crossing the
/// scan can see, so flooring it at a second would cap that guarantee at
/// ~6.6 km of track. Only a zero or negative step has to be excluded.
const MIN_AOI_STEP: Duration = Duration::milliseconds(1);

/// A region on Earth's surface a satellite can be tasked against.
///
/// Implemented here by [`Polygon`], [`Rectangle`] and [`Circle`]. Implement
/// it on your own type to detect windows over a shape this crate does not
/// provide.
pub trait Area {
    /// Signed angular offset of `point` from this area's boundary, in radians:
    /// positive inside, negative outside, exactly zero on the boundary.
    ///
    /// The magnitude must never *exceed* the true angular distance from
    /// `point` to the nearest boundary point. Window detection relies on that
    /// bound to guarantee it cannot step over a crossing. It is deliberately
    /// **not** required to equal that distance, nor to be continuous — only
    /// the sign and the bound matter.
    ///
    /// A non-zero [`AoiIterOpts::max_off_nadir`] compares the magnitude
    /// against the field of regard rather than against zero, so how tight the
    /// bound is sets how precise the window edges are. All three built-in
    /// areas report the exact distance.
    fn signed_angular_offset(&self, point: LatLon) -> Radians;

    /// Angular distance from `point` to the *farthest* point of this area, in
    /// radians.
    ///
    /// The mirror of [`signed_angular_offset`](Area::signed_angular_offset)'s
    /// contract: this must never fall *below* the true distance, and must not
    /// change faster than `point` moves. Only [`Coverage::Full`] reads it, and
    /// an over-estimate costs coverage windows rather than the step guarantee.
    ///
    /// The supplied implementation needs no override for an area whose
    /// `signed_angular_offset` is exact and continuous, which all three
    /// built-ins are. It works because the farthest point of the area from
    /// `point` is the nearest one to `point`'s antipode: `π − d(antipode)`.
    /// An area that under-reports its offset inherits an over-estimate here,
    /// which is the safe direction for the magnitude.
    ///
    /// Continuity is the part that does not carry over.
    /// `signed_angular_offset` is explicitly not required to be continuous,
    /// and the default inherits any jump it has — but the step guarantee for
    /// [`Coverage::Full`] rests on this changing no faster than `point` moves,
    /// not merely on the bound. **An `Area` with a discontinuous offset must
    /// supply its own `max_angular_distance` to be used with
    /// [`Coverage::Full`].**
    fn max_angular_distance(&self, point: LatLon) -> Radians {
        let antipode = LatLon::new(
            Degrees(-point.latitude.to_f64()),
            Degrees(point.longitude.to_f64() + 180.0),
        );
        // A positive offset puts the antipode inside the area, so the area
        // reaches all the way round and the farthest point is a full π away.
        Radians(PI + self.signed_angular_offset(antipode).to_f64().min(0.0))
    }
}

impl<A: Area + ?Sized> Area for &A {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        (**self).signed_angular_offset(point)
    }

    fn max_angular_distance(&self, point: LatLon) -> Radians {
        (**self).max_angular_distance(point)
    }
}

/// How the interior of a self-intersecting [`Polygon`] is determined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum FillRule {
    /// Inside wherever the winding number is non-zero. A ring that crosses
    /// itself stays filled.
    #[default]
    NonZero,
    /// Inside wherever the winding number is odd. A ring that doubles back on
    /// itself leaves a hole.
    EvenOdd,
}

/// A closed polygon on Earth's surface whose edges are great-circle arcs.
///
/// The ring closes implicitly, joining the last vertex back to the first —
/// repeating the first vertex at the end is accepted and ignored. Vertex order
/// does not matter: a reversed ring describes the same area.
///
/// # Examples
///
/// ```
/// use sgp4_predict::{Degrees, LatLon, Polygon};
///
/// // A box over Scotland.
/// let area = Polygon::new([
///     LatLon { latitude: Degrees(54.5), longitude: Degrees(-6.5) },
///     LatLon { latitude: Degrees(54.5), longitude: Degrees(-1.5) },
///     LatLon { latitude: Degrees(59.0), longitude: Degrees(-1.5) },
///     LatLon { latitude: Degrees(59.0), longitude: Degrees(-6.5) },
/// ])?;
///
/// // `(latitude, longitude)` tuples convert, when the order is unambiguous
/// // from nearby context.
/// let same = Polygon::new([
///     (Degrees(54.5), Degrees(-6.5)),
///     (Degrees(54.5), Degrees(-1.5)),
///     (Degrees(59.0), Degrees(-1.5)),
///     (Degrees(59.0), Degrees(-6.5)),
/// ])?;
/// # Ok::<(), sgp4_predict::Error>(())
/// ```
///
/// `==` compares the vertex list, not the region: the same ring listed from a
/// different starting vertex, or in the opposite direction, compares unequal
/// even though both cover the same area.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    /// Vertices as unit vectors, deduplicated, ring closing implicitly.
    verts: Vec<[f64; 3]>,
    /// Per-edge great-circle normals, `normalize(vᵢ × vᵢ₊₁)`.
    normals: Vec<[f64; 3]>,
    /// Axis of a spherical cap containing the whole boundary.
    cap_axis: [f64; 3],
    /// Angular radius of that cap, always `< π/2`.
    cap_radius: f64,
    fill: FillRule,
}

impl Polygon {
    /// Build a polygon from vertices in ring order — [`LatLon`] values, or
    /// anything converting to one, including `(latitude, longitude)` tuples.
    ///
    /// Consecutive duplicates are dropped, so an explicitly repeated closing
    /// vertex is harmless. At least three distinct vertices must remain.
    ///
    /// # Errors
    ///
    /// - [`Error::Latitude`] if a latitude is outside `[-90, 90]`.
    /// - [`Error::NotFinite`] if a longitude is NaN or infinite. Longitude
    ///   itself is unbounded — it wraps — so only finiteness is checked.
    /// - [`Error::TooFewVertices`] if fewer than three distinct vertices remain.
    /// - [`Error::AntipodalEdge`] if consecutive vertices are antipodal, since
    ///   no unique great-circle arc joins them.
    /// - [`Error::LargerThanHemisphere`] if the polygon does not fit inside a
    ///   hemisphere. See the type documentation for why.
    pub fn new(vertices: impl IntoIterator<Item = impl Into<LatLon>>) -> Result<Self> {
        let mut verts: Vec<[f64; 3]> = Vec::new();
        for vertex in vertices {
            let vertex = vertex.into();
            let lat = vertex.latitude.to_f64();
            if !(-90.0..=90.0).contains(&lat) {
                return Err(Error::Latitude(lat).into());
            }
            // A non-finite longitude gives a NaN vertex, which slips past every
            // comparison below and reaches `normalize` as a zero-norm vector.
            let lon = vertex.longitude.to_f64();
            if !lon.is_finite() {
                return Err(Error::NotFinite {
                    what: "polygon vertex longitude",
                    value: lon,
                }
                .into());
            }
            let v = unit_from_lat_lon(vertex);
            if verts.last().is_none_or(|prev| !coincident(*prev, v)) {
                verts.push(v);
            }
        }
        // The implicit closing edge makes a first/last duplicate redundant too.
        if verts.len() > 1 && coincident(verts[0], *verts.last().expect("len > 1")) {
            verts.pop();
        }
        if verts.len() < 3 {
            return Err(Error::TooFewVertices(verts.len()).into());
        }

        let mut normals = Vec::with_capacity(verts.len());
        for (index, (&a, &b)) in verts.iter().zip(cycled(&verts)).enumerate() {
            if dot(a, b) < -1.0 + COINCIDENT {
                return Err(Error::AntipodalEdge { index }.into());
            }
            normals.push(normalize(cross(a, b)).expect("non-coincident, non-antipodal"));
        }

        // Centroid axis. It is not the minimal enclosing cap, so the
        // hemisphere check below is conservative — a polygon that would fit
        // under an optimally placed cap may still be rejected.
        let sum = verts.iter().fold([0.0; 3], |acc, v| {
            [acc[0] + v[0], acc[1] + v[1], acc[2] + v[2]]
        });
        let cap_axis = normalize(sum).unwrap_or(verts[0]);
        let cap_radius = verts
            .iter()
            .map(|&v| angle_between(cap_axis, v))
            .fold(0.0, f64::max);
        if cap_radius >= FRAC_PI_2 - COINCIDENT {
            return Err(Error::LargerThanHemisphere {
                radius_deg: Radians(cap_radius).degrees(),
            }
            .into());
        }

        Ok(Self {
            verts,
            normals,
            cap_axis,
            cap_radius,
            fill: FillRule::default(),
        })
    }

    /// Set how a self-intersecting ring's interior is determined. Has no
    /// effect on a simple (non-self-intersecting) polygon.
    #[must_use = "returns a reconfigured Polygon; the receiver is unchanged"]
    pub fn with_fill_rule(mut self, fill: FillRule) -> Self {
        self.fill = fill;
        self
    }

    /// The polygon's vertices in ring order, after deduplication.
    #[must_use]
    pub fn vertices(&self) -> impl DoubleEndedIterator<Item = LatLon> + ExactSizeIterator + '_ {
        self.verts.iter().map(|&v| lat_lon_from_unit(v))
    }

    /// Winding number of the boundary about `p`, via the signed angle it
    /// subtends in the tangent plane at `p`.
    ///
    /// Only meaningful for `p` inside the bounding cap: this measures degree
    /// on the sphere minus `{p, -p}`, so it also returns a non-zero count at
    /// the *antipode* of the polygon. The caller must have excluded that.
    fn winding(&self, p: [f64; 3]) -> i64 {
        let mut sum = 0.0;
        for (&a, &b) in self.verts.iter().zip(cycled(&self.verts)) {
            let ta = reject(a, p);
            let tb = reject(b, p);
            sum += dot(cross(ta, tb), p).atan2(dot(ta, tb));
        }
        (sum / TAU).round() as i64
    }
}

impl Area for Polygon {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        let p = unit_from_lat_lon(point);

        // The bounding cap settles the sign on its own: `winding` is valid
        // only once the antipode is known to be outside the region, which is
        // exactly what containment in the cap guarantees. The magnitude still
        // comes from the edge loop below, so it is the true distance either
        // way.
        let outside_cap = angle_between(self.cap_axis, p) > self.cap_radius + COINCIDENT;

        let mut d = f64::INFINITY;
        for ((&a, &b), &n) in self
            .verts
            .iter()
            .zip(cycled(&self.verts))
            .zip(&self.normals)
        {
            // Endpoints are always candidates, and the perpendicular foot only
            // when it lies within the arc. Including the endpoints
            // unconditionally keeps the result an under-estimate even if the
            // in-arc test flips at its own boundary, where the two agree.
            d = d.min(angle_between(p, a)).min(angle_between(p, b));

            let pn = dot(p, n);
            let foot = reject(p, n);
            if dot(cross(a, foot), n) >= 0.0 && dot(cross(foot, b), n) >= 0.0 {
                d = d.min(pn.abs().clamp(0.0, 1.0).asin());
            }
        }

        if outside_cap {
            return Radians(-d);
        }

        // On the boundary. Returning early also keeps `winding` away from the
        // only configuration where its tangent-plane projections degenerate.
        if d < ON_BOUNDARY {
            return Radians(0.0);
        }

        let k = self.winding(p);
        let inside = match self.fill {
            FillRule::NonZero => k != 0,
            FillRule::EvenOdd => k % 2 != 0,
        };
        Radians(if inside { d } else { -d })
    }
}

/// A latitude/longitude box.
///
/// Unlike a four-vertex [`Polygon`], the north and south edges follow their
/// parallels **exactly** — no great-circle bulge — so `Rectangle` is what you
/// want whenever the region really is "these latitudes by these longitudes".
///
/// # Examples
///
/// ```
/// use sgp4_predict::{Degrees, LatLon, Rectangle};
///
/// let scotland = Rectangle::new(
///     LatLon { latitude: Degrees(54.0), longitude: Degrees(-8.0) },
///     LatLon { latitude: Degrees(60.0), longitude: Degrees(-1.0) },
/// )?;
///
/// // The box runs eastward from the south-west corner, so a north-east corner
/// // west of it wraps across the antimeridian.
/// let pacific = Rectangle::new(
///     (Degrees(-20.0), Degrees(160.0)),
///     (Degrees(20.0), Degrees(-160.0)),
/// )?;
///
/// // Bands and polar caps span every longitude.
/// let arctic = Rectangle::latitude_band(Degrees(66.5), Degrees(90.0))?;
/// # Ok::<(), sgp4_predict::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Rectangle {
    south: f64,
    north: f64,
    west: f64,
    /// Longitude extent eastward from `west`, in `(0, 2π]`.
    lon_span: f64,
    /// `None` when the box spans every longitude, so it has no side edges.
    sides: Option<Sides>,
}

#[derive(Debug, Clone, PartialEq)]
struct Sides {
    corners: [[f64; 3]; 4],
    meridians: [Meridian; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Meridian {
    /// Normal of the meridian's great-circle plane.
    normal: [f64; 3],
    /// Point on the equator at this longitude. Distinguishes this meridian
    /// from the antimeridian sharing its plane.
    equator: [f64; 3],
}

impl Rectangle {
    /// Build a box from its south-west and north-east corners.
    ///
    /// The box runs **eastward** from the south-west corner, so a north-east
    /// corner at a smaller longitude wraps across the antimeridian. Use
    /// [`latitude_band`](Rectangle::latitude_band) for a box spanning every
    /// longitude.
    ///
    /// # Errors
    ///
    /// - [`Error::Latitude`] if a latitude is outside `[-90, 90]`.
    /// - [`Error::NotFinite`] if a longitude is NaN or infinite. Longitude
    ///   itself is unbounded — it wraps — so only finiteness is checked.
    /// - [`Error::EmptyRectangle`] if the box has no extent in either axis.
    pub fn new(south_west: impl Into<LatLon>, north_east: impl Into<LatLon>) -> Result<Self> {
        let (sw, ne) = (south_west.into(), north_east.into());
        let (south, north) = (
            checked_latitude(sw.latitude)?,
            checked_latitude(ne.latitude)?,
        );
        let west = wrap_pi(checked_angle(sw.longitude, "rectangle west longitude")?);
        let east = wrap_pi(checked_angle(ne.longitude, "rectangle east longitude")?);
        let lon_span = match wrap_tau(east - west) {
            // The corners share a longitude. Read as zero width rather than
            // full width; a full-width box goes through `latitude_band`.
            span if span < COINCIDENT => 0.0,
            span => span,
        };
        Self::build(south, north, west, lon_span)
    }

    /// Build a box spanning every longitude between two latitudes — a band, or
    /// a polar cap when one latitude is a pole.
    pub fn latitude_band(south: Degrees, north: Degrees) -> Result<Self> {
        Self::build(checked_latitude(south)?, checked_latitude(north)?, -PI, TAU)
    }

    fn build(south: f64, north: f64, west: f64, lon_span: f64) -> Result<Self> {
        if north - south < COINCIDENT || lon_span < COINCIDENT {
            return Err(Error::EmptyRectangle {
                south: Radians(south).degrees(),
                north: Radians(north).degrees(),
            }
            .into());
        }

        let sides = (lon_span < TAU - COINCIDENT).then(|| {
            let east = west + lon_span;
            let corner = |lat: f64, lon: f64| unit_from_radians(lat, lon);
            Sides {
                corners: [
                    corner(south, west),
                    corner(south, east),
                    corner(north, east),
                    corner(north, west),
                ],
                meridians: [meridian(west), meridian(east)],
            }
        });

        // Dropping the sides has to widen the span to match. Left as given, a
        // near-full box keeps a sliver that `contains` excludes but no edge
        // measures, so a point in it reports the distance to the nearest
        // parallel — an over-report, which the contract forbids.
        let lon_span = if sides.is_some() { lon_span } else { TAU };

        Ok(Self {
            south,
            north,
            west,
            lon_span,
            sides,
        })
    }

    /// The southern and northern latitude bounds.
    #[must_use]
    pub fn latitudes(&self) -> (Degrees, Degrees) {
        (
            Radians(self.south).to_degrees(),
            Radians(self.north).to_degrees(),
        )
    }

    /// The western bound and the extent eastward from it. The extent is 360°
    /// for a box built by [`latitude_band`](Rectangle::latitude_band).
    #[must_use]
    pub fn longitudes(&self) -> (Degrees, Degrees) {
        (
            Radians(self.west).to_degrees(),
            Radians(self.lon_span).to_degrees(),
        )
    }

    /// Strict on both axes: a point within `ON_BOUNDARY` of an edge never
    /// reaches this test, having already short-circuited to zero.
    fn contains(&self, lat: f64, lon: f64) -> bool {
        (self.south..=self.north).contains(&lat) && wrap_tau(lon - self.west) <= self.lon_span
    }
}

impl Area for Rectangle {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        let lat = point.latitude.radians();
        let lon = point.longitude.radians();
        let p = unit_from_lat_lon(point);

        let mut d = f64::INFINITY;

        // North and south edges are parallels, so the distance to them is the
        // latitude difference measured along a meridian — exact, with none of
        // a great circle's bulge. It only applies within the longitude range:
        // a point at the same latitude but half a world away is close to the
        // *parallel*, not to this box. A bound at a pole is not an edge at all
        // — the parallel there is a single point, interior to the box unless
        // meridian edges meet at it, and those are handled as corners below.
        if self.sides.is_none() || wrap_tau(lon - self.west) <= self.lon_span {
            if self.south > -FRAC_PI_2 + COINCIDENT {
                d = d.min((lat - self.south).abs());
            }
            if self.north < FRAC_PI_2 - COINCIDENT {
                d = d.min((self.north - lat).abs());
            }
        }

        if let Some(sides) = &self.sides {
            for &c in &sides.corners {
                d = d.min(angle_between(p, c));
            }
            for m in &sides.meridians {
                let foot = reject(p, m.normal);
                // Reject the antimeridian half of the same plane, then keep
                // only feet that land within the edge's latitude span. Without
                // both checks a point on the far side of the Earth would
                // report a near-zero distance to this edge.
                if dot(foot, m.equator) <= 0.0 {
                    continue;
                }
                let foot_lat = match normalize(foot) {
                    Some(f) => f[2].clamp(-1.0, 1.0).asin(),
                    None => continue,
                };
                if (self.south..=self.north).contains(&foot_lat) {
                    d = d.min(dot(p, m.normal).abs().clamp(0.0, 1.0).asin());
                }
            }
        }

        // A pole-to-pole band has no edge of any kind, leaving `d` infinite. π
        // is the widest separation on a sphere, so the clamp under-reports,
        // which the contract permits; every other box is already below it.
        let d = d.min(PI);

        if d < ON_BOUNDARY {
            return Radians(0.0);
        }
        Radians(if self.contains(lat, lon) { d } else { -d })
    }
}

/// A circular area on Earth's surface — a spherical cap.
///
/// The radius is **angular**, like every other measurement here. A degree of
/// arc is about 111.2 km on the ground, so a 250 km radius is roughly
/// `Degrees(2.25)`.
///
/// # Examples
///
/// ```
/// use sgp4_predict::{Circle, Degrees, LatLon};
///
/// // A circular area 500 km across.
/// let cape_town = Circle::new(
///     LatLon { latitude: Degrees(-33.9), longitude: Degrees(18.4) },
///     Degrees(2.25),
/// )?;
///
/// // `(latitude, longitude)` tuples convert too.
/// let north_sea = Circle::new((Degrees(56.0), Degrees(2.0)), Degrees(2.7))?;
/// # Ok::<(), sgp4_predict::Error>(())
/// ```
///
/// For an elongated or oriented region, use a [`Polygon`].
#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    centre: [f64; 3],
    /// Angular radius, in `(0, π/2)`.
    radius: f64,
}

impl Circle {
    /// Build a circular area from its centre and angular radius.
    ///
    /// # Errors
    ///
    /// - [`Error::Latitude`] if the centre's latitude is outside `[-90, 90]`.
    /// - [`Error::NotFinite`] if the centre's longitude or the radius is NaN
    ///   or infinite. Longitude itself is unbounded — it wraps — so only
    ///   finiteness is checked.
    /// - [`Error::CircleRadius`] unless the radius is in `(0, 90°)`.
    pub fn new(centre: impl Into<LatLon>, radius: Degrees) -> Result<Self> {
        let centre = centre.into();
        checked_latitude(centre.latitude)?;
        checked_angle(centre.longitude, "circle centre longitude")?;

        let r = checked_angle(radius, "circle radius")?;
        if !(r > 0.0 && r < FRAC_PI_2 - COINCIDENT) {
            return Err(Error::CircleRadius {
                radius_deg: radius.to_f64(),
            }
            .into());
        }

        Ok(Self {
            centre: unit_from_lat_lon(centre),
            radius: r,
        })
    }

    /// The circle's centre.
    #[must_use]
    pub fn centre(&self) -> LatLon {
        lat_lon_from_unit(self.centre)
    }

    /// The circle's angular radius.
    #[must_use]
    pub fn radius(&self) -> Degrees {
        Radians(self.radius).to_degrees()
    }
}

impl Area for Circle {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        let d = self.radius - angle_between(self.centre, unit_from_lat_lon(point));
        if d.abs() < ON_BOUNDARY {
            return Radians(0.0);
        }
        Radians(d)
    }
}

/// The window during which an [`Area`] is within the payload's reach.
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators to cover a specific
/// overpass, and [`TimeWindow`](crate::TimeWindow) for
/// [`clamp_to`](crate::TimeWindow::clamp_to).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AoiWindow {
    /// When the area comes within reach.
    pub start: DateTime<Utc>,
    /// When it passes back out of reach.
    pub end: DateTime<Utc>,
}

impl AoiWindow {
    /// Build a window from its entry and exit times.
    #[must_use]
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }
}

impl time::IntervalRange for AoiWindow {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

impl time::TimeWindow for AoiWindow {
    fn with_bounds(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self::new(start, end)
    }
}

/// Whether any part of an [`Area`], or all of it, must be within reach for a
/// window to be open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Coverage {
    /// Any part of the area is within reach. The window opens as the nearest
    /// part of the area enters the field of regard and closes as the last part
    /// leaves.
    #[default]
    Any,
    /// Every part of the area is within reach at once.
    ///
    /// Needs a [`max_off_nadir`](AoiIterOpts::max_off_nadir) wider than the
    /// area: at the default of zero the reach is a point, so no window ever
    /// opens. An area wider than the field of regard yields nothing for the
    /// same reason.
    ///
    /// This is not the same as "one image covers the area" — that depends on
    /// the instantaneous field of view, and an area wider than a single swath
    /// has to be broken into strips, which is outside this crate's scope.
    Full,
}

/// The field of regard `max_central_angle` is safe to be handed.
///
/// A negative angle is no cone at all, and at or past π/2 the sine stops
/// growing so the coverage relation runs backwards. NaN maps to zero rather
/// than propagating: it survives `f64::clamp`, and `f64::min` then swallows it
/// inside `max_central_angle`, which would silently report the full horizon —
/// every line-of-sight pass an access window, with no error raised anywhere.
fn resolve_off_nadir(max_off_nadir: Radians) -> f64 {
    let angle = max_off_nadir.to_f64();
    if angle.is_nan() {
        return 0.0;
    }
    angle.clamp(0.0, FRAC_PI_2 - COINCIDENT)
}

/// Central angle from the sub-satellite point to the farthest ground point a
/// payload slewed to `max_off_nadir` can reach, for a satellite at geocentric
/// radius `r` over local Earth radius `re`.
///
/// The standard coverage relation, from the triangle joining Earth's centre,
/// the satellite and the target. Monotone in `max_off_nadir`, and clamped at
/// the horizon — which is also the implicit line-of-sight check, since a cone
/// wider than the horizon reaches no further than it.
fn max_central_angle(max_off_nadir: f64, r: f64, re: f64) -> f64 {
    let horizon = (re / r).clamp(-1.0, 1.0).acos();
    let sin_horizon_angle = (r / re) * max_off_nadir.sin();
    if sin_horizon_angle >= 1.0 {
        horizon
    } else {
        (sin_horizon_angle.asin() - max_off_nadir).min(horizon)
    }
}

/// Event function: the area's signed angular offset from the edge of what the
/// payload can reach. Positive when in reach, so the windows where it is
/// positive are the access opportunities.
///
/// No rate is supplied: the offset is not differentiable across the medial
/// axis or a vertex bisector, which is exactly the geometry that matters here,
/// and the bracketed solver converges on the time bracket regardless.
pub(crate) struct AreaInView<'a, A: Area> {
    predictor: Predictor,
    area: &'a A,
    /// Clamped to `[0, π/2)` on construction.
    max_off_nadir: f64,
    coverage: Coverage,
}

// `A` is only ever held behind a shared reference, so a derive's `A: Debug` /
// `A: Clone` bounds would be a false requirement on caller-supplied areas.
impl<A: Area> fmt::Debug for AreaInView<'_, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AreaInView")
            .field("predictor", &self.predictor)
            .field("max_off_nadir", &self.max_off_nadir)
            .field("coverage", &self.coverage)
            .finish_non_exhaustive()
    }
}

impl<A: Area> Clone for AreaInView<'_, A> {
    fn clone(&self) -> Self {
        Self {
            predictor: self.predictor.clone(),
            area: self.area,
            max_off_nadir: self.max_off_nadir,
            coverage: self.coverage,
        }
    }
}

impl<'a, A: Area> AreaInView<'a, A> {
    fn new(predictor: Predictor, area: &'a A, opts: &AoiIterOpts) -> Self {
        Self {
            predictor,
            area,
            max_off_nadir: resolve_off_nadir(opts.max_off_nadir),
            coverage: opts.coverage,
        }
    }
}

impl<'a, A: Area> EventFunction for AreaInView<'a, A> {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        let ecef = self.predictor.propagate(t)?.to_ecef(t);
        let geodetic = ecef.to_geodetic();
        let position = ecef.position;
        let r =
            (position.x * position.x + position.y * position.y + position.z * position.z).sqrt();
        // `r - altitude` is the local Earth radius along the geodetic normal
        // rather than along the radius vector; the two differ by ~3 m at LEO.
        let reach = max_central_angle(self.max_off_nadir, r, r - geodetic.altitude);

        let point = geodetic.into();
        let value = match self.coverage {
            Coverage::Any => self.area.signed_angular_offset(point).to_f64() + reach,
            Coverage::Full => reach - self.area.max_angular_distance(point).to_f64(),
        };
        Ok(Sample {
            time: t,
            value,
            rate: None,
        })
    }
}

/// Adaptive stepping that cannot step over a boundary crossing.
///
/// [`Area`] guarantees `|value|` never exceeds the true angular distance to
/// the boundary, and the ground point's angular speed never exceeds
/// `angular_rate`, so the boundary cannot be *reached* in less than
/// `|value| / angular_rate` seconds. Stepping by that is therefore safe no
/// matter how narrow the area — unlike a fixed step, which can jump clean over
/// a short chord.
///
/// A non-zero `max_off_nadir` does not weaken this. Both forms the event
/// function takes remain bounded by the distance to their own zero set: for
/// [`Coverage::Any`] the reach shifts the boundary outward by the same amount
/// in both signs, and for [`Coverage::Full`] the bound follows from
/// [`Area::max_angular_distance`] changing no faster than the point moves.
///
/// The `min` floor is the one exception, and the reason it exists: without it
/// the step collapses to zero at the boundary and the scan stalls. A chord the
/// ground track traverses in less than `min` can still be missed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ProximityStep {
    min: Duration,
    max: Duration,
    /// Upper bound on the ground point's angular speed, rad/s.
    angular_rate: f64,
}

impl StepStrategy for ProximityStep {
    fn next_time(&mut self, current: DateTime<Utc>, sample: Option<&Sample>) -> DateTime<Utc> {
        let step = match sample {
            Some(s) => {
                // Clamping the f64 milliseconds before the Duration conversion
                // keeps it from overflowing and already bounds the result to
                // self.max; only the floor still needs enforcing. It also
                // funnels a NaN value into `min` rather than a stall, since
                // clamp propagates NaN and the cast then saturates to zero.
                let millis = (s.value.abs() / self.angular_rate * 1e3)
                    .clamp(0.0, self.max.num_milliseconds() as f64);
                Duration::milliseconds(millis as i64).max(self.min)
            }
            None => self.max,
        };
        current + step
    }
}

/// Upper bound on the angular speed of the sub-satellite point, in rad/s.
///
/// Derived from the element set rather than measured, so it is a true bound
/// rather than the largest value some sampling happened to see:
///
/// - `n √(1−e²) / (1−e)²` is the two-body maximum angular rate about Earth's
///   centre, attained at perigee (from `h = n a² √(1−e²)` and `r_p = a(1−e)`).
/// - `+ ω_E` because the ground point is in ECEF, where
///   `ṗ = ṗ_ECI − ω_E × p`.
/// - `× 1/(1−e²_WGS84)` because mapping geodetic latitude onto the sphere
///   stretches latitude by at most that; longitude is unstretched.
/// - `× 1.05` covers SGP4's osculating-versus-mean discrepancy and the small
///   contribution of radial motion to geodetic latitude. It also absorbs the
///   drift of `max_central_angle` with altitude, which for LEO runs three
///   orders of magnitude below the ground point's own rate.
fn max_sub_point_rate(elements: &Elements) -> f64 {
    /// Earth's sidereal rotation rate (rad/s), WGS-84.
    const OMEGA_EARTH: f64 = 7.292_115_0e-5;
    const LAT_STRETCH: f64 = 1.0 / (1.0 - WGS84_E2);
    const SAFETY: f64 = 1.05;

    let n = elements.mean_motion * TAU / 86_400.0;
    let e = elements.eccentricity.clamp(0.0, 0.999);
    let perigee_rate = n * (1.0 - e * e).sqrt() / ((1.0 - e) * (1.0 - e));

    (SAFETY * LAT_STRETCH * (perigee_rate + OMEGA_EARTH)).max(1e-6)
}

/// Tuning knobs for [`AoiIter`]'s coarse scan and window walk.
///
/// Pass a customised value to
/// [`Predictor::aoi_iter_with_opts`](crate::Predictor::aoi_iter_with_opts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AoiIterOpts {
    /// Half-angle of the satellite's **field of regard** — the largest nadir
    /// angle the payload can be slewed to — measured from the position vector.
    ///
    /// Zero, the default, detects the sub-satellite point itself crossing into
    /// the area. Raising it opens the window as soon as the area comes within
    /// reach of the payload, which for a 30° field of regard at ISS altitude is
    /// about 2.2° of arc, or 245 km.
    ///
    /// This is a field of *regard*, not of view: it describes everything the
    /// payload could be pointed at, not the footprint of a single image.
    /// Clamped to `[0, 90°)`; a non-finite value is taken as zero.
    ///
    /// [`Coverage::Full`] needs this set wider than the area, since at zero
    /// the reach is a single point.
    pub max_off_nadir: Radians,
    /// Whether any part of the area or all of it must be within reach.
    pub coverage: Coverage,
    /// Lower bound of the adaptive coarse-scan step. Also the shortest
    /// crossing the scan is guaranteed to see, so lower it for an area the
    /// ground track can cross in under a second. Floored at 1 ms.
    pub min_step: Duration,
    /// Upper bound of the adaptive coarse-scan step, used when the ground
    /// track is far from the area. Raised to `min_step` if it is below it.
    pub max_step: Duration,
    /// Fixed step used to walk from a window's start to its end.
    ///
    /// Unlike the coarse scan this has no skip guarantee, so for a **concave**
    /// area a notch the ground track leaves and re-enters within `walk_step`
    /// is absorbed into the surrounding window. A convex area is unaffected.
    ///
    /// Floored at 1 s, unlike `min_step`. That does not limit which windows are
    /// found: the walk brackets outward from a coarse-scan sample already known
    /// to be inside, and both ends are refined, so a sub-second window is still
    /// resolved exactly. It bounds only the notch width above.
    pub walk_step: Duration,
    /// A window longer than this is reported as
    /// [`DetectError::WindowTooLong`](crate::DetectError::WindowTooLong).
    /// Raise it for a continental-scale area.
    pub max_window_duration: Duration,
    /// A window already in progress at the interval start is discarded by
    /// default; set to `false` to instead walk backward past the interval
    /// start and find its true beginning.
    pub skip_leading_partial: bool,
    /// A window still in progress at the interval end is walked forward past
    /// the interval to find its true end by default; set to `true` to instead
    /// clamp it to the interval bounds.
    pub clamp_to_interval: bool,
}

impl Default for AoiIterOpts {
    fn default() -> Self {
        Self {
            max_off_nadir: Radians(0.0),
            coverage: Coverage::Any,
            min_step: Duration::seconds(1),
            max_step: Duration::minutes(10),
            walk_step: Duration::seconds(5),
            max_window_duration: Duration::hours(1),
            skip_leading_partial: true,
            clamp_to_interval: false,
        }
    }
}

/// The coarse-scan bounds an [`AoiIterOpts`] actually yields: `min_step`
/// floored, and `max_step` raised to it rather than to a fixed constant, so a
/// wholly sub-second pair is honoured as asked.
fn step_bounds(opts: &AoiIterOpts) -> (Duration, Duration) {
    let min = opts.min_step.max(MIN_AOI_STEP);
    (min, opts.max_step.max(min))
}

/// Iterator over the windows during which an area is within the payload's
/// reach.
///
/// Created by [`Predictor::aoi_iter`](crate::Predictor::aoi_iter).
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct AoiIter<'a, A: Area> {
    inner: WindowIter<AreaInView<'a, A>, ProximityStep>,
}

impl<A: Area> fmt::Debug for AoiIter<'_, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AoiIter")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<A: Area> Clone for AoiIter<'_, A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<'a, A: Area> AoiIter<'a, A> {
    /// Scan `interval` for windows over `area`. Prefer
    /// [`Predictor::aoi_iter`](crate::Predictor::aoi_iter), which supplies the
    /// defaults.
    pub fn new(
        predictor: Predictor,
        area: &'a A,
        interval: impl time::IntervalRange,
        opts: AoiIterOpts,
        refinement: Refinement,
    ) -> Self {
        let (min, max) = step_bounds(&opts);
        let step = ProximityStep {
            min,
            max,
            angular_rate: max_sub_point_rate(&predictor.elements),
        };
        let mut builder = WindowIter::builder()
            .interval(interval)
            .event_function(AreaInView::new(predictor, area, &opts))
            .step(step)
            .walk_step(opts.walk_step)
            .max_window_duration(opts.max_window_duration)
            .refinement(refinement);
        if !opts.skip_leading_partial {
            builder = builder.include_leading_partial();
        }
        if opts.clamp_to_interval {
            builder = builder.clamp_to_interval();
        }
        let inner = builder.build().expect("interval is always supplied");
        Self { inner }
    }
}

impl<'a, A: Area> Iterator for AoiIter<'a, A> {
    type Item = Result<AoiWindow>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.inner.next()?.map(|window| {
            let window = AoiWindow::new(window.start, window.end);
            tracing::debug!(entry = %window.start, exit = %window.end, "aoi window detected");
            window
        }))
    }
}

impl Predictor {
    /// Find every window in which `area` is within the payload's reach.
    ///
    /// Reach is set by [`AoiIterOpts::max_off_nadir`], which defaults to zero
    /// — the sub-satellite point itself crossing into the area. See
    /// [`Predictor::aoi_iter_with_opts`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use chrono::{Duration, Utc};
    /// use sgp4_predict::{Degrees, Polygon, Predictor, Tle};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let tle: Tle = unimplemented!();
    /// let predictor = Predictor::from_tle(tle)?;
    /// let area = Polygon::new([
    ///     (Degrees(54.0), Degrees(-8.0)),
    ///     (Degrees(54.0), Degrees(-1.0)),
    ///     (Degrees(60.0), Degrees(-1.0)),
    ///     (Degrees(60.0), Degrees(-8.0)),
    /// ])?;
    ///
    /// let start = Utc::now();
    /// for window in predictor.aoi_iter(&area, start..start + Duration::days(1)) {
    ///     let window = window?;
    ///     println!("over the area from {} to {}", window.start, window.end);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn aoi_iter<'a, A: Area>(
        &self,
        area: &'a A,
        interval: impl IntervalRange,
    ) -> AoiIter<'a, A> {
        self.aoi_iter_with_opts(area, interval, AoiIterOpts::default(), self.refinement)
    }

    /// Like [`Predictor::aoi_iter`], but with a customized root-finder
    /// configuration and coarse-scan/window-walk tuning. See [`Refinement`]
    /// and [`AoiIterOpts`].
    pub fn aoi_iter_with_opts<'a, A: Area>(
        &self,
        area: &'a A,
        interval: impl IntervalRange,
        opts: AoiIterOpts,
        refinement: Refinement,
    ) -> AoiIter<'a, A> {
        AoiIter::new(self.clone(), area, interval, opts, refinement)
    }

    /// Detect whether `area` is within the payload's reach at time `t`.
    ///
    /// Returns `Ok(None)` if it is not. Otherwise walks backward and
    /// forward from `t` using [`AoiIterOpts::default`]'s `walk_step` to find
    /// the entry and exit crossings, refining each with the bracketed hybrid
    /// solver ([`Refinement`]).
    ///
    /// Returns [`Error::Detect`](crate::Error::Detect) if the window is longer
    /// than [`AoiIterOpts::default`]'s `max_window_duration`.
    pub fn detect_aoi<A: Area>(&self, t: DateTime<Utc>, area: &A) -> Result<Option<AoiWindow>> {
        self.detect_aoi_with_opts(t, area, AoiIterOpts::default())
    }

    /// Like [`Predictor::detect_aoi`], but with a customized field of regard,
    /// walk step and max window duration. Only
    /// [`AoiIterOpts::max_off_nadir`], [`AoiIterOpts::coverage`],
    /// [`AoiIterOpts::walk_step`] and [`AoiIterOpts::max_window_duration`] are
    /// used — the other fields don't apply to this single-point detection.
    pub fn detect_aoi_with_opts<A: Area>(
        &self,
        t: DateTime<Utc>,
        area: &A,
        opts: AoiIterOpts,
    ) -> Result<Option<AoiWindow>> {
        let mut f = AreaInView::new(self.clone(), area, &opts);
        let window = detect::detect_window(
            &mut f,
            t,
            opts.walk_step,
            opts.max_window_duration,
            &self.refinement,
        )?;
        Ok(window.map(|w| {
            let window = AoiWindow::new(w.start, w.end);
            tracing::debug!(entry = %window.start, exit = %window.end, "aoi window detected");
            window
        }))
    }
}

/// Errors from constructing an [`Area`].
#[derive(Debug, Clone, PartialEq, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// Fewer than three distinct vertices remained after deduplication.
    #[error("polygon needs at least 3 distinct vertices, got {0}")]
    TooFewVertices(usize),
    /// A latitude fell outside `[-90, 90]`.
    #[error("latitude {0} is outside [-90, 90]")]
    Latitude(f64),
    /// An angle was NaN or infinite. `what` names the argument it came from.
    #[error("{what} is not finite: {value}")]
    NotFinite {
        /// Which argument was non-finite.
        what: &'static str,
        /// The offending value.
        value: f64,
    },
    /// Two consecutive vertices are antipodal, so no unique great-circle arc
    /// joins them.
    #[error("polygon edge {index} joins antipodal vertices; no unique great-circle arc joins them")]
    AntipodalEdge {
        /// Index of the edge's first vertex.
        index: usize,
    },
    /// The polygon does not fit inside a hemisphere. See [`Polygon`].
    #[error(
        "polygon spans {radius_deg:.1}° from its centre and does not fit within a hemisphere; \
         split it into smaller polygons, or describe the complementary region instead"
    )]
    LargerThanHemisphere {
        /// Angular radius of the smallest cap on the centroid axis that
        /// contains the ring.
        radius_deg: f64,
    },
    /// The box has no extent in latitude, in longitude, or in both.
    #[error(
        "rectangle is empty: south {south}° must lie below north {north}°, and the corners \
         must differ in longitude — note that -180° and 180° are the same meridian, so use \
         `Rectangle::latitude_band` for a box spanning every longitude"
    )]
    EmptyRectangle {
        /// The southern bound, in degrees.
        south: f64,
        /// The northern bound, in degrees.
        north: f64,
    },
    /// The radius fell outside `(0, 90°)`.
    #[error("circle radius must lie in (0, 90°), got {radius_deg}°")]
    CircleRadius {
        /// The offending radius, in degrees.
        radius_deg: f64,
    },
}

// --- unit-sphere helpers -------------------------------------------------
//
// Deliberately not added to `vectors.rs`: `Vec3<K, F>` is tagged with a
// coordinate frame and a kind for the TEME/ECEF/ENU pipeline, while these are
// dimensionless directions on a unit sphere. Keeping them apart is what stops
// the two being mixed.

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = norm(a);
    (n > f64::MIN_POSITIVE).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// Component of `a` perpendicular to the unit vector `u`.
fn reject(a: [f64; 3], u: [f64; 3]) -> [f64; 3] {
    let s = dot(a, u);
    [a[0] - s * u[0], a[1] - s * u[1], a[2] - s * u[2]]
}

/// Angle between two unit vectors. `atan2` of the cross and dot magnitudes,
/// not `acos`, which loses all precision for nearly parallel inputs — which is
/// exactly the regime near a boundary.
fn angle_between(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

fn coincident(a: [f64; 3], b: [f64; 3]) -> bool {
    angle_between(a, b) < COINCIDENT
}

/// The slice rotated one place left, for iterating `(vᵢ, vᵢ₊₁)` edge pairs
/// around a closed ring.
fn cycled(v: &[[f64; 3]]) -> impl Iterator<Item = &[f64; 3]> {
    v.iter().skip(1).chain(v.iter().take(1))
}

/// Map geodetic latitude/longitude onto the unit sphere, using geodetic
/// latitude directly as spherical latitude.
///
/// This is what fixes the meaning of a polygon edge (see the module docs). The
/// same map is applied to vertices and to the ground point, so containment is
/// exact — the sign of an offset is never affected by the convention.
///
/// The *magnitude* is stretched relative to a true geocentric central angle,
/// by `d(geocentric)/d(geodetic) = (1−e²)/(cos²φ + (1−e²)² sin²φ)` in the
/// north-south direction: 0.9933 at the equator, 1.0067 at a pole, and
/// unstretched east-west. That is invisible at `max_off_nadir` zero, where
/// only the sign matters, and is a real term once the magnitude is compared
/// against a reach — up to 0.67%, so about 0.015° (1.6 km) at a 2.2° reach.
/// Comparable to the geocentric-nadir convention and an order below TLE error.
fn unit_from_lat_lon(p: LatLon) -> [f64; 3] {
    unit_from_radians(p.latitude.radians(), p.longitude.radians())
}

fn unit_from_radians(lat: f64, lon: f64) -> [f64; 3] {
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();
    [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat]
}

/// The great-circle plane through both poles at longitude `lon`, paired with
/// the equator point that tells its two halves apart.
fn meridian(lon: f64) -> Meridian {
    let (sin_lon, cos_lon) = lon.sin_cos();
    Meridian {
        normal: [-sin_lon, cos_lon, 0.0],
        equator: [cos_lon, sin_lon, 0.0],
    }
}

fn checked_latitude(lat: Degrees) -> Result<f64> {
    if !(-90.0..=90.0).contains(&lat.to_f64()) {
        return Err(Error::Latitude(lat.to_f64()).into());
    }
    Ok(lat.radians())
}

/// Reject a non-finite angle, converting to radians. A NaN slips past every
/// comparison below — `NaN < COINCIDENT` is false — so it would be built into
/// the shape and make every offset NaN.
fn checked_angle(angle: Degrees, what: &'static str) -> Result<f64> {
    let value = angle.to_f64();
    if !value.is_finite() {
        return Err(Error::NotFinite { what, value }.into());
    }
    Ok(angle.radians())
}

/// Wrap an angle to `[-π, π)`.
fn wrap_pi(x: f64) -> f64 {
    x - TAU * ((x + PI) / TAU).floor()
}

/// Wrap an angle to `[0, 2π)`.
fn wrap_tau(x: f64) -> f64 {
    x.rem_euclid(TAU)
}

fn lat_lon_from_unit(v: [f64; 3]) -> LatLon {
    LatLon {
        latitude: Radians(v[2].clamp(-1.0, 1.0).asin()).to_degrees(),
        longitude: Radians(v[1].atan2(v[0])).to_degrees(),
    }
}

#[cfg(test)]
mod rectangle_tests {
    use super::*;
    use crate::Error;

    fn scotland() -> Rectangle {
        Rectangle::new(
            (Degrees(54.0), Degrees(-8.0)),
            (Degrees(60.0), Degrees(-1.0)),
        )
        .expect("valid box")
    }

    fn offset(r: &Rectangle, lat: f64, lon: f64) -> f64 {
        r.signed_angular_offset(LatLon::new(Degrees(lat), Degrees(lon)))
            .to_f64()
    }

    /// The whole reason `Rectangle` exists: its north and south edges sit on
    /// their parallels exactly, where a four-vertex `Polygon` bulges away.
    #[test]
    fn test_edges_follow_parallels_exactly() {
        let rect = scotland();
        let poly = Polygon::new([
            (Degrees(54.0), Degrees(-8.0)),
            (Degrees(54.0), Degrees(-1.0)),
            (Degrees(60.0), Degrees(-1.0)),
            (Degrees(60.0), Degrees(-8.0)),
        ])
        .expect("valid ring");

        // Mid-edge, a hair north of 60°N: outside the rectangle by definition.
        let (lat, lon) = (60.001, -4.5);
        assert!(
            offset(&rect, lat, lon) < 0.0,
            "rectangle must not extend north of its stated latitude"
        );
        // The polygon's great-circle edge bows ~0.046° north here, so the same
        // point falls inside it.
        assert!(
            poly.signed_angular_offset(LatLon::new(Degrees(lat), Degrees(lon)))
                .to_f64()
                > 0.0,
            "the polygon edge is expected to bulge past the parallel"
        );

        // Every point on the parallel reads as exactly on the boundary.
        for i in 0..=70 {
            let lon = -8.0 + 7.0 * i as f64 / 70.0;
            assert!(
                offset(&rect, 60.0, lon).abs() < 1e-12,
                "60°N at {lon}° should be on the boundary"
            );
            assert!(
                offset(&rect, 54.0, lon).abs() < 1e-12,
                "54°N at {lon}° should be on the boundary"
            );
        }
    }

    #[test]
    fn test_contains_and_excludes() {
        let rect = scotland();
        assert!(offset(&rect, 57.0, -4.5) > 0.0);
        for (lat, lon) in [(53.9, -4.5), (60.1, -4.5), (57.0, -8.1), (57.0, -0.9)] {
            assert!(
                offset(&rect, lat, lon) < 0.0,
                "({lat}, {lon}) should be outside"
            );
        }
    }

    /// Nothing on the far side of the Earth may report a near-zero offset —
    /// a meridian's plane extends round the globe, and its antipodal half must
    /// not be mistaken for the edge.
    #[test]
    fn test_far_side_of_the_earth_is_far() {
        let rect = scotland();
        for lat in [-80.0, -57.0, 0.0, 57.0, 80.0] {
            for lon in [172.0, 175.5, 179.0, -180.0, -175.0] {
                let v = offset(&rect, lat, lon);
                assert!(v < 0.0, "({lat}, {lon}) should be outside");
                assert!(
                    v.abs() > 0.5,
                    "({lat}, {lon}) reported only {v} rad from the box"
                );
            }
        }
    }

    #[test]
    fn test_offset_never_exceeds_true_distance() {
        let rect = scotland();
        // Dense boundary sample: the two parallels and the two meridians.
        let mut boundary = Vec::new();
        for i in 0..=2_000 {
            let f = i as f64 / 2_000.0;
            boundary.push(super::unit_from_radians(
                Degrees(54.0).radians(),
                Degrees(-8.0 + 7.0 * f).radians(),
            ));
            boundary.push(super::unit_from_radians(
                Degrees(60.0).radians(),
                Degrees(-8.0 + 7.0 * f).radians(),
            ));
            boundary.push(super::unit_from_radians(
                Degrees(54.0 + 6.0 * f).radians(),
                Degrees(-8.0).radians(),
            ));
            boundary.push(super::unit_from_radians(
                Degrees(54.0 + 6.0 * f).radians(),
                Degrees(-1.0).radians(),
            ));
        }

        for p in super::geometry_tests::sphere_points(500) {
            let ll = lat_lon_from_unit(p);
            let reported = rect.signed_angular_offset(ll).to_f64().abs();
            let truth = boundary
                .iter()
                .map(|&b| angle_between(p, b))
                .fold(f64::INFINITY, f64::min);
            assert!(
                reported <= truth + 1e-6,
                "reported {reported} exceeds true distance {truth} at {ll:?}"
            );
        }
    }

    #[test]
    fn test_antimeridian_box_wraps_eastward() {
        let rect = Rectangle::new(
            (Degrees(-20.0), Degrees(160.0)),
            (Degrees(20.0), Degrees(-160.0)),
        )
        .expect("valid box");

        for lon in [160.0, 175.0, 180.0, -180.0, -170.0, -160.0] {
            assert!(offset(&rect, 0.0, lon) >= 0.0, "lon {lon} should be inside");
        }
        for lon in [159.0, 0.0, -159.0] {
            assert!(offset(&rect, 0.0, lon) < 0.0, "lon {lon} should be outside");
        }
    }

    #[test]
    fn test_latitude_band_and_polar_cap() {
        let band = Rectangle::latitude_band(Degrees(-10.0), Degrees(10.0)).expect("valid band");
        for lon in [-180.0, -90.0, 0.0, 90.0, 179.0] {
            assert!(offset(&band, 0.0, lon) > 0.0, "equator at {lon} is inside");
            assert!(offset(&band, 20.0, lon) < 0.0, "20°N at {lon} is outside");
        }
        // Inside a band, the distance is purely the latitude difference.
        assert!((offset(&band, 5.0, 42.0) - Degrees(5.0).radians()).abs() < 1e-12);

        let cap = Rectangle::latitude_band(Degrees(66.5), Degrees(90.0)).expect("valid cap");
        assert!(offset(&cap, 90.0, 0.0) > 0.0, "the pole is inside the cap");
        assert!(offset(&cap, 70.0, 123.0) > 0.0);
        assert!(offset(&cap, 60.0, 123.0) < 0.0);
    }

    /// A span within `COINCIDENT` of full loses its side edges, so it has to
    /// lose the sliver between them too — nothing is left to measure the
    /// distance to it, and a point there would report the distance to the
    /// nearest parallel instead.
    #[test]
    fn test_near_full_span_widens_to_a_band() {
        let rect = Rectangle::new(
            (Degrees(-10.0), Degrees(0.0)),
            (Degrees(10.0), Degrees(-1e-8)),
        )
        .expect("valid box");

        let (_, span) = rect.longitudes();
        assert!((span.to_f64() - 360.0).abs() < 1e-12, "got {span:?}");
        for lon in [-1e-9, -0.5e-8, 0.0, 90.0, -179.0] {
            let v = offset(&rect, 0.0, lon);
            assert!(v > 0.0, "lon {lon} should be inside, got {v}");
        }
    }

    /// A pole-to-pole band is the one box with no boundary at all, so nothing
    /// constrains the distance to it. The reported offset must still be a
    /// number: an infinity would be baked into every sample.
    #[test]
    fn test_whole_sphere_band_is_finite() {
        let all = Rectangle::latitude_band(Degrees(-90.0), Degrees(90.0)).expect("valid band");
        for (lat, lon) in [(0.0, 0.0), (90.0, 0.0), (-90.0, 45.0), (57.0, -179.0)] {
            let v = offset(&all, lat, lon);
            assert!(v.is_finite(), "({lat}, {lon}) reported {v}");
            assert!(v > 0.0, "({lat}, {lon}) should be inside");
        }
    }

    #[test]
    fn test_pole_to_pole_wedge() {
        // A lune. `Polygon` rejects this as larger than a hemisphere, but a
        // rectangle needs no such restriction: containment is exact.
        let wedge = Rectangle::new(
            (Degrees(-90.0), Degrees(0.0)),
            (Degrees(90.0), Degrees(90.0)),
        )
        .expect("valid wedge");

        assert!(offset(&wedge, 0.0, 45.0) > 0.0);
        assert!(offset(&wedge, 60.0, 45.0) > 0.0);
        assert!(offset(&wedge, 0.0, -45.0) < 0.0);
        assert!(offset(&wedge, 0.0, 135.0) < 0.0);

        // A pole is where the two meridian edges meet, so it is a boundary
        // point however the wedge is entered. `dot(foot, equator)` is
        // identically zero there and skips both meridians, but the corners
        // stand in: at a latitude bound of ±90° every corner *is* the pole.
        for lon in [0.0, 45.0, 90.0, -170.0] {
            assert!(
                offset(&wedge, 90.0, lon).abs() < 1e-12,
                "north pole at {lon}° should read as on the boundary"
            );
            assert!(
                offset(&wedge, -90.0, lon).abs() < 1e-12,
                "south pole at {lon}° should read as on the boundary"
            );
        }

        // The same holds when only one bound is a pole.
        let north = Rectangle::new((Degrees(0.0), Degrees(0.0)), (Degrees(90.0), Degrees(90.0)))
            .expect("valid wedge");
        assert!(offset(&north, 90.0, 45.0).abs() < 1e-12);
    }

    #[test]
    fn test_empty_and_invalid_rectangles() {
        // South at or above north.
        assert!(matches!(
            Rectangle::new(
                (Degrees(60.0), Degrees(0.0)),
                (Degrees(54.0), Degrees(10.0))
            ),
            Err(Error::Aoi(super::Error::EmptyRectangle { .. }))
        ));
        // Zero height.
        assert!(matches!(
            Rectangle::new(
                (Degrees(54.0), Degrees(0.0)),
                (Degrees(54.0), Degrees(10.0))
            ),
            Err(Error::Aoi(super::Error::EmptyRectangle { .. }))
        ));
        // Zero width.
        assert!(matches!(
            Rectangle::new((Degrees(54.0), Degrees(5.0)), (Degrees(60.0), Degrees(5.0))),
            Err(Error::Aoi(super::Error::EmptyRectangle { .. }))
        ));
        // -180° and 180° are the same meridian, so this reads as zero width
        // rather than full width. The error has to say so — it is where
        // someone writing a band the obvious way actually lands.
        let err = Rectangle::new(
            (Degrees(-10.0), Degrees(-180.0)),
            (Degrees(10.0), Degrees(180.0)),
        )
        .expect_err("±180° is a single meridian");
        assert!(matches!(
            err,
            Error::Aoi(super::Error::EmptyRectangle { .. })
        ));
        assert!(
            err.to_string().contains("latitude_band"),
            "the error should point at the band constructor: {err}"
        );
        // Out-of-range latitude.
        assert!(matches!(
            Rectangle::new(
                (Degrees(-91.0), Degrees(0.0)),
                (Degrees(60.0), Degrees(10.0))
            ),
            Err(Error::Aoi(super::Error::Latitude(_)))
        ));
    }

    /// A non-finite latitude fails the range test; a non-finite longitude has
    /// no range to fail, so it needs its own check. Unchecked, a NaN reaches
    /// `signed_angular_offset` and every sample is NaN, which `ProximityStep`
    /// floors to `min_step` — the whole interval scanned at a millisecond,
    /// with no error ever surfacing.
    #[test]
    fn test_non_finite_coordinates_rejected() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                matches!(
                    Rectangle::new(
                        (Degrees(54.0), Degrees(bad)),
                        (Degrees(60.0), Degrees(-1.0))
                    ),
                    Err(Error::Aoi(super::Error::NotFinite { .. }))
                ),
                "west longitude {bad} was not rejected"
            );
            assert!(
                matches!(
                    Rectangle::new(
                        (Degrees(54.0), Degrees(-8.0)),
                        (Degrees(60.0), Degrees(bad))
                    ),
                    Err(Error::Aoi(super::Error::NotFinite { .. }))
                ),
                "east longitude {bad} was not rejected"
            );
            assert!(
                matches!(
                    Rectangle::new(
                        (Degrees(bad), Degrees(-8.0)),
                        (Degrees(60.0), Degrees(-1.0))
                    ),
                    Err(Error::Aoi(super::Error::Latitude(_)))
                ),
                "south latitude {bad} was not rejected"
            );
            assert!(
                matches!(
                    Rectangle::latitude_band(Degrees(-10.0), Degrees(bad)),
                    Err(Error::Aoi(super::Error::Latitude(_)))
                ),
                "band latitude {bad} was not rejected"
            );
        }
    }

    #[test]
    fn test_accessors_round_trip() {
        let rect = scotland();
        let (south, north) = rect.latitudes();
        assert!((south.to_f64() - 54.0).abs() < 1e-12);
        assert!((north.to_f64() - 60.0).abs() < 1e-12);
        let (west, span) = rect.longitudes();
        assert!((west.to_f64() - -8.0).abs() < 1e-12);
        assert!((span.to_f64() - 7.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod circle_tests {
    use super::*;
    use crate::Error;

    fn cape_town() -> Circle {
        Circle::new((Degrees(-33.9), Degrees(18.4)), Degrees(2.25)).expect("valid circle")
    }

    /// The offset is the exact signed distance to the boundary everywhere, not
    /// merely the lower bound `Area` asks for.
    #[test]
    fn test_offset_is_the_exact_signed_distance() {
        let circle = cape_town();
        let centre = circle.centre();
        assert!((centre.latitude.to_f64() + 33.9).abs() < 1e-12);
        assert!((centre.longitude.to_f64() - 18.4).abs() < 1e-12);
        assert!((circle.radius().to_f64() - 2.25).abs() < 1e-12);

        let c = unit_from_lat_lon(centre);
        for p in super::geometry_tests::sphere_points(500) {
            let truth = circle.radius().radians() - angle_between(c, p);
            let reported = circle.signed_angular_offset(lat_lon_from_unit(p)).to_f64();
            assert!(
                (reported - truth).abs() < 1e-12,
                "offset should equal {truth} at {:?}, got {reported}",
                lat_lon_from_unit(p)
            );
        }
    }

    #[test]
    fn test_invalid_radius_rejected() {
        let centre = (Degrees(56.0), Degrees(2.0));
        for bad in [0.0, -1.0, 90.0, 120.0] {
            let err = Circle::new(centre, Degrees(bad)).expect_err("rejected");
            assert!(
                matches!(err, Error::Aoi(super::Error::CircleRadius { .. })),
                "radius {bad} gave {err}"
            );
        }
        assert!(matches!(
            Circle::new((Degrees(91.0), Degrees(0.0)), Degrees(1.0)).expect_err("bad latitude"),
            Error::Aoi(super::Error::Latitude(_))
        ));
    }

    /// The latitude range test rejects a non-finite latitude on its own; the
    /// longitude and the radius have no range that does. Unchecked, a NaN is
    /// built into the centre and every offset is NaN, which `ProximityStep`
    /// floors to `min_step` — the whole interval scanned at a millisecond,
    /// with no error ever surfacing.
    #[test]
    fn test_non_finite_arguments_rejected() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for (label, err) in [
                (
                    "centre longitude",
                    Circle::new((Degrees(56.0), Degrees(bad)), Degrees(2.25)),
                ),
                (
                    "radius",
                    Circle::new((Degrees(56.0), Degrees(2.0)), Degrees(bad)),
                ),
            ] {
                assert!(
                    matches!(err, Err(Error::Aoi(super::Error::NotFinite { .. }))),
                    "{label} {bad} was not rejected"
                );
            }
            assert!(
                matches!(
                    Circle::new((Degrees(bad), Degrees(2.0)), Degrees(1.0)),
                    Err(Error::Aoi(super::Error::Latitude(_)))
                ),
                "centre latitude {bad} was not rejected"
            );
        }
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    /// WGS-84 equatorial radius and an ISS-like altitude, in metres.
    const RE: f64 = 6_378_137.0;
    const R: f64 = RE + 420_000.0;

    fn reach(off_nadir_deg: f64) -> f64 {
        Radians(max_central_angle(Degrees(off_nadir_deg).radians(), R, RE)).degrees()
    }

    /// A zero field of regard reaches only the sub-satellite point itself.
    #[test]
    fn test_zero_off_nadir_reaches_nowhere() {
        assert_eq!(reach(0.0), 0.0);
    }

    /// Hand-checked against `asin((r/re) sin η) − η`.
    #[test]
    fn test_matches_the_coverage_relation() {
        assert!(
            (reach(30.0) - 2.203_297).abs() < 1e-6,
            "got {}",
            reach(30.0)
        );
        assert!(
            (reach(45.0) - 3.909_269).abs() < 1e-6,
            "got {}",
            reach(45.0)
        );
    }

    /// At grazing incidence the line of sight leaves the Earth tangentially,
    /// and `asin` has no solution past it. The reach stops at the horizon
    /// instead of going undefined.
    #[test]
    fn test_clamped_at_the_horizon() {
        let horizon = Radians((RE / R).acos()).degrees();
        // Grazing is at asin(re/r) ≈ 69.75° for this altitude.
        assert!(reach(69.0) < horizon);
        for off_nadir in [69.8, 70.0, 80.0, 89.9] {
            assert!(
                (reach(off_nadir) - horizon).abs() < 1e-9,
                "{off_nadir}° reached {}, horizon is {horizon}",
                reach(off_nadir)
            );
        }
    }

    /// NaN survives `f64::clamp`, and `f64::min` swallows it inside
    /// `max_central_angle`, so an unguarded NaN reports the full horizon —
    /// every line-of-sight pass an access window, with no error raised.
    #[test]
    fn test_non_finite_off_nadir_never_reaches_the_relation() {
        assert_eq!(resolve_off_nadir(Radians(f64::NAN)), 0.0);
        assert_eq!(resolve_off_nadir(Radians(f64::NEG_INFINITY)), 0.0);
        assert_eq!(
            resolve_off_nadir(Radians(f64::INFINITY)),
            FRAC_PI_2 - COINCIDENT
        );
        assert_eq!(resolve_off_nadir(Radians(-1.0)), 0.0);

        // The bug the guard exists for: unclamped, this is the full horizon.
        let horizon = max_central_angle(0.0, R, RE);
        assert!((max_central_angle(f64::NAN, R, RE) - horizon).abs() > 0.3);
        assert_eq!(
            max_central_angle(resolve_off_nadir(Radians(f64::NAN)), R, RE),
            0.0
        );
    }

    #[test]
    fn test_monotone_in_off_nadir() {
        let mut previous = 0.0;
        for i in 1..=69 {
            let r = reach(f64::from(i));
            assert!(r > previous, "reach fell at {i}°: {r} <= {previous}");
            previous = r;
        }
    }

    /// The supplied `max_angular_distance` reads the offset at the antipode,
    /// so check it against a brute-force maximum over each area's boundary.
    #[test]
    fn test_max_angular_distance_matches_brute_force() {
        let polygon = Polygon::new([
            (Degrees(40.0), Degrees(-10.0)),
            (Degrees(40.0), Degrees(30.0)),
            (Degrees(65.0), Degrees(30.0)),
            (Degrees(65.0), Degrees(-10.0)),
        ])
        .expect("valid polygon");
        // Wide enough that the farthest point of the north edge is mid-parallel
        // rather than a corner.
        let rectangle = Rectangle::new(
            (Degrees(40.0), Degrees(-60.0)),
            (Degrees(65.0), Degrees(60.0)),
        )
        .expect("valid rectangle");
        let circle =
            Circle::new((Degrees(52.0), Degrees(10.0)), Degrees(10.0)).expect("valid circle");

        let interior_of = |area: &dyn Area| {
            // The area as a point cloud, from the offset's sign alone, so the
            // reference needs nothing from `max_angular_distance` itself.
            geometry_tests::sphere_points(60_000)
                .into_iter()
                .filter(|&p| area.signed_angular_offset(lat_lon_from_unit(p)).to_f64() >= 0.0)
                .collect::<Vec<_>>()
        };

        for (label, area) in [
            ("polygon", &polygon as &dyn Area),
            ("rectangle", &rectangle as &dyn Area),
            ("circle", &circle as &dyn Area),
        ] {
            let interior = interior_of(area);
            assert!(!interior.is_empty(), "{label} sampled empty");
            for p in geometry_tests::sphere_points(200) {
                let reported = area.max_angular_distance(lat_lon_from_unit(p)).to_f64();
                let truth = interior
                    .iter()
                    .map(|&q| angle_between(p, q))
                    .fold(0.0, f64::max);
                assert!(
                    reported >= truth - 1e-9,
                    "{label} reported {reported} below the true farthest {truth}"
                );
            }
        }
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use chrono::TimeZone;

    fn step(value: Option<f64>) -> Duration {
        step_within(Duration::seconds(1), Duration::minutes(10), value)
    }

    fn step_within(min: Duration, max: Duration, value: Option<f64>) -> Duration {
        let now = Utc.with_ymd_and_hms(2025, 12, 20, 12, 0, 0).unwrap();
        let mut s = ProximityStep {
            min,
            max,
            // A round number near a typical LEO ground-track rate.
            angular_rate: 1e-3,
        };
        let sample = value.map(|value| Sample {
            time: now,
            value,
            rate: None,
        });
        s.next_time(now, sample.as_ref()) - now
    }

    #[test]
    fn test_no_sample_takes_the_largest_step() {
        assert_eq!(step(None), Duration::minutes(10));
    }

    #[test]
    fn test_far_from_the_boundary_takes_the_largest_step() {
        // 1 radian away: over 15 minutes of travel, so capped at max.
        assert_eq!(step(Some(1.0)), Duration::minutes(10));
        assert_eq!(step(Some(-1.0)), Duration::minutes(10));
    }

    #[test]
    fn test_step_scales_with_distance_to_the_boundary() {
        // 0.06 rad at 1e-3 rad/s is 60 s away, inside or outside.
        assert_eq!(step(Some(0.06)), Duration::seconds(60));
        assert_eq!(step(Some(-0.06)), Duration::seconds(60));
    }

    #[test]
    fn test_at_the_boundary_takes_the_smallest_step() {
        assert_eq!(step(Some(0.0)), Duration::seconds(1));
        assert_eq!(step(Some(1e-12)), Duration::seconds(1));
    }

    #[test]
    fn test_nan_value_still_advances() {
        // A NaN must floor to `min`, not stall the scan.
        assert_eq!(step(Some(f64::NAN)), Duration::seconds(1));
    }

    #[test]
    fn test_sub_second_bounds_are_honoured() {
        // The whole point of the knob: a 100 ms floor is what lets the scan
        // see a chord it crosses in under a second.
        assert_eq!(
            step_within(
                Duration::milliseconds(100),
                Duration::milliseconds(500),
                Some(0.0)
            ),
            Duration::milliseconds(100)
        );
        assert_eq!(
            step_within(
                Duration::milliseconds(100),
                Duration::milliseconds(500),
                Some(1.0)
            ),
            Duration::milliseconds(500)
        );
    }

    #[test]
    fn test_fractional_max_step_is_not_truncated() {
        // Clamping in whole seconds would round this cap down to 1 s.
        assert_eq!(
            step_within(
                Duration::milliseconds(1),
                Duration::milliseconds(1500),
                Some(1.0)
            ),
            Duration::milliseconds(1500)
        );
    }

    // --- step_bounds ---

    #[test]
    fn test_step_bounds_floor_at_a_millisecond() {
        let bounds = |min, max| {
            step_bounds(&AoiIterOpts {
                min_step: min,
                max_step: max,
                ..Default::default()
            })
        };

        // A sub-second request survives; only zero and negative are floored.
        assert_eq!(
            bounds(Duration::milliseconds(100), Duration::minutes(10)).0,
            Duration::milliseconds(100)
        );
        assert_eq!(
            bounds(Duration::zero(), Duration::minutes(10)).0,
            Duration::milliseconds(1)
        );
        assert_eq!(
            bounds(Duration::seconds(-5), Duration::minutes(10)).0,
            Duration::milliseconds(1)
        );

        // `max` is raised to `min`, not to a fixed constant, so a wholly
        // sub-second pair stays sub-second.
        assert_eq!(
            bounds(Duration::milliseconds(100), Duration::milliseconds(500)),
            (Duration::milliseconds(100), Duration::milliseconds(500))
        );
        assert_eq!(
            bounds(Duration::milliseconds(100), Duration::milliseconds(50)),
            (Duration::milliseconds(100), Duration::milliseconds(100))
        );
    }

    // --- max_sub_point_rate ---

    /// Restate a TLE line with its trailing checksum recomputed, so the test
    /// data below can be edited without hand-arithmetic.
    fn with_checksum(line: &str) -> String {
        let body: String = line.chars().take(68).collect();
        let sum: u32 = body
            .chars()
            .map(|c| match c {
                '0'..='9' => c.to_digit(10).expect("matched a digit"),
                '-' => 1,
                _ => 0,
            })
            .sum();
        format!("{body}{}", sum % 10)
    }

    /// The canonical SENTINEL-2C element set with its eccentricity replaced.
    /// `eccentricity` is the 7-digit implied-decimal field on line 2.
    fn elements_with(eccentricity: &str) -> Elements {
        let line1 =
            with_checksum("1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990");
        let line2 = with_checksum(&format!(
            "2 60989  98.5671  69.0082 {eccentricity}  95.1447 264.9872 14.30821394 67740"
        ));
        Elements::from_tle(None, line1.as_bytes(), line2.as_bytes()).expect("valid tle")
    }

    #[test]
    fn test_max_sub_point_rate_exceeds_the_circular_rate() {
        // A near-circular orbit at 14.308 rev/day turns at 2π·14.308/86400
        // rad/s. The bound must sit above that.
        let circular = 14.30824 * TAU / 86_400.0;
        let rate = max_sub_point_rate(&elements_with("0001000"));
        assert!(
            rate > circular,
            "bound {rate} does not exceed the circular rate {circular}"
        );
    }

    #[test]
    fn test_max_sub_point_rate_grows_with_eccentricity() {
        // Perigee passage of an eccentric orbit is far faster, so the bound
        // must loosen accordingly rather than staying near the mean rate. At
        // e = 0.7 the analytic factor `√(1−e²)/(1−e)²` is ≈ 7.9.
        let ratio = max_sub_point_rate(&elements_with("7000000"))
            / max_sub_point_rate(&elements_with("0001000"));
        assert!(
            ratio > 5.0,
            "an eccentric orbit must be bounded far more loosely, got {ratio:.2}×"
        );
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    use crate::Error;
    use crate::angle::Degrees;

    /// The octant triangle: three mutually perpendicular vertices, so every
    /// edge is a quarter great circle and the geometry is exactly checkable by
    /// hand. Its centroid sits `asin(1/√3)` ≈ 35.26° from all three edges.
    fn octant() -> Polygon {
        Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(90.0)),
            (Degrees(90.0), Degrees(0.0)),
        ])
        .expect("valid triangle")
    }

    /// A 10° box over Scotland — the shape a caller would actually write.
    fn scotland() -> Polygon {
        Polygon::new([
            (Degrees(54.0), Degrees(-8.0)),
            (Degrees(54.0), Degrees(-1.0)),
            (Degrees(60.0), Degrees(-1.0)),
            (Degrees(60.0), Degrees(-8.0)),
        ])
        .expect("valid box")
    }

    /// Note the round-trip: the vector reaching the polygon is
    /// `unit_from_lat_lon(lat_lon_from_unit(v))`, not `v` itself, so these are
    /// not exact-input tests. The tolerances below absorb the difference.
    fn offset(poly: &Polygon, v: [f64; 3]) -> f64 {
        poly.signed_angular_offset(lat_lon_from_unit(v)).to_f64()
    }

    fn at(lat: f64, lon: f64) -> [f64; 3] {
        unit_from_lat_lon(LatLon::new(Degrees(lat), Degrees(lon)))
    }

    /// Great-circle interpolation from `a` to `b`.
    fn slerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
        let theta = angle_between(a, b);
        let (s0, s1) = ((1.0 - t) * theta, t * theta);
        let (sa, sb) = (s0.sin() / theta.sin(), s1.sin() / theta.sin());
        normalize([
            sa * a[0] + sb * b[0],
            sa * a[1] + sb * b[1],
            sa * a[2] + sb * b[2],
        ])
        .expect("interpolant is non-degenerate")
    }

    /// Densely sampled points on the polygon's boundary.
    fn boundary(poly: &Polygon, per_edge: usize) -> Vec<[f64; 3]> {
        let mut out = Vec::with_capacity(poly.verts.len() * per_edge);
        for (&a, &b) in poly.verts.iter().zip(cycled(&poly.verts)) {
            for i in 0..per_edge {
                out.push(slerp(a, b, i as f64 / per_edge as f64));
            }
        }
        out
    }

    /// True angular distance from `p` to the polygon's boundary, to machine
    /// precision.
    ///
    /// Not a `min` over sampled boundary points: that *over*-estimates by up
    /// to half the sample spacing near the boundary — 2e-4 rad on `octant()`'s
    /// 90° edges, which would swamp any tolerance worth asserting — and
    /// closing that by sampling alone needs ~800k points per edge. Each edge
    /// is minimised directly instead. Distance along a great-circle arc is
    /// `cos d = cos d₀ cos(s − s₀)`, so on an arc under 180° it has a single
    /// interior minimum or none, which is what ternary search needs; the
    /// endpoints are included for the case where the extremum is a maximum.
    fn distance_to_boundary(poly: &Polygon, p: [f64; 3]) -> f64 {
        let mut best = f64::INFINITY;
        for (&a, &b) in poly.verts.iter().zip(cycled(&poly.verts)) {
            let f = |t: f64| angle_between(p, slerp(a, b, t));
            let (mut lo, mut hi) = (0.0, 1.0);
            // (2/3)^100 is far below f64 resolution on [0, 1].
            for _ in 0..100 {
                let third = (hi - lo) / 3.0;
                if f(lo + third) < f(hi - third) {
                    hi -= third;
                } else {
                    lo += third;
                }
            }
            best = best.min(f(0.0)).min(f(1.0)).min(f(0.5 * (lo + hi)));
        }
        best
    }

    /// Deterministic uniform points on the sphere, so failures reproduce.
    pub(super) fn sphere_points(n: usize) -> Vec<[f64; 3]> {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1_u64 << 53) as f64
        };
        (0..n)
            .map(|_| {
                let z: f64 = 2.0 * next() - 1.0;
                let phi = TAU * next();
                let r = (1.0 - z * z).max(0.0).sqrt();
                [r * phi.cos(), r * phi.sin(), z]
            })
            .collect()
    }

    // --- the contract the safe step rests on ---

    /// `Area` promises the magnitude never exceeds the true angular distance
    /// to the boundary. Window detection turns that bound into "a crossing can
    /// never be stepped over", so it is the single most important property
    /// here — and it must hold on the bounding-cap branch too, which
    /// deliberately under-reports.
    #[test]
    fn test_offset_never_exceeds_true_distance() {
        for poly in [octant(), scotland()] {
            for p in sphere_points(500) {
                let reported = offset(&poly, p).abs();
                let truth = distance_to_boundary(&poly, p);
                // A real bound on the over-report, not the discretisation error
                // of the reference. Worst observed is ~2e-16, so the slack here
                // is for the `offset` helper's unit-vector round-trip.
                assert!(
                    reported <= truth + 1e-9,
                    "reported {reported} exceeds true distance {truth} at {p:?}"
                );
            }
        }
    }

    /// The tangent-plane angle sum measures degree on the sphere minus
    /// `{p, -p}`, so without the bounding-cap gate the *antipode* of the
    /// polygon also winds to ±1 and reads as inside. Roughly half the detected
    /// windows would be on the wrong side of the Earth.
    #[test]
    fn test_antipode_of_polygon_is_outside() {
        for poly in [octant(), scotland()] {
            for p in boundary(&poly, 50)
                .into_iter()
                .chain(std::iter::once(poly.cap_axis))
            {
                let antipode = [-p[0], -p[1], -p[2]];
                assert!(
                    offset(&poly, antipode) < 0.0,
                    "antipode of {p:?} reported as inside"
                );
            }
        }
    }

    // --- sign and magnitude ---

    #[test]
    fn test_centroid_is_inside_at_the_expected_distance() {
        let poly = octant();
        let c = normalize([1.0, 1.0, 1.0]).unwrap();
        let expected = (1.0_f64 / 3.0).sqrt().asin();
        let got = offset(&poly, c);
        assert!(
            (got - expected).abs() < 1e-12,
            "expected {expected}, got {got}"
        );
    }

    #[test]
    fn test_boundary_points_are_zero() {
        for poly in [octant(), scotland()] {
            for p in boundary(&poly, 97) {
                assert!(
                    offset(&poly, p).abs() < 1e-9,
                    "boundary point {p:?} reported offset {}",
                    offset(&poly, p)
                );
            }
        }
    }

    #[test]
    fn test_reversed_ring_is_identical() {
        let forward = scotland();
        let reversed = Polygon::new(forward.vertices().rev().map(|g| (g.latitude, g.longitude)))
            .expect("valid box");
        for p in sphere_points(300) {
            assert!(
                (offset(&forward, p) - offset(&reversed, p)).abs() < 1e-12,
                "vertex order changed the result at {p:?}"
            );
        }
    }

    // --- the vertex cases the design exists to handle ---

    /// A ground track passing exactly through a vertex must cross the sign
    /// boundary once and stay there — no oscillation, no repeated crossings.
    #[test]
    fn test_track_through_vertex_crosses_once() {
        let poly = octant();
        let a = poly.verts[0];
        let inward = normalize(reject(normalize([1.0, 1.0, 1.0]).unwrap(), a)).unwrap();

        let mut changes = 0;
        let mut prev: Option<bool> = None;
        for i in 0..=20_000 {
            let theta = -0.3 + 0.6 * i as f64 / 20_000.0;
            let p = [
                theta.cos() * a[0] + theta.sin() * inward[0],
                theta.cos() * a[1] + theta.sin() * inward[1],
                theta.cos() * a[2] + theta.sin() * inward[2],
            ];
            let inside = offset(&poly, p) >= 0.0;
            if prev.is_some_and(|q| q != inside) {
                changes += 1;
            }
            prev = Some(inside);
        }
        assert_eq!(changes, 1, "expected exactly one crossing at the vertex");
    }

    /// Passing just outside a vertex must not register as entering.
    #[test]
    fn test_near_tangential_graze_at_vertex_stays_outside() {
        let poly = octant();
        let a = poly.verts[0];
        let bisector = normalize([0.0, 1.0, 1.0]).unwrap();
        let eps = 1e-9;
        // A point `eps` outside the vertex along the interior bisector.
        let base = normalize([
            a[0] - eps * bisector[0],
            a[1] - eps * bisector[1],
            a[2] - eps * bisector[2],
        ])
        .unwrap();
        // Travel perpendicular to the bisector, so the excursion skims the
        // vertex without entering the wedge.
        let along = normalize([0.0, 1.0, -1.0]).unwrap();

        for i in 0..=2_000 {
            let s = -1e-6 + 2e-6 * i as f64 / 2_000.0;
            let p = normalize([
                base[0] + s * along[0],
                base[1] + s * along[1],
                base[2] + s * along[2],
            ])
            .unwrap();
            let v = offset(&poly, p);
            assert!(v.is_finite(), "non-finite offset at s={s}");
            assert!(v < 0.0, "graze at s={s} reported inside (offset {v})");
        }
    }

    // --- awkward shapes ---

    #[test]
    fn test_thin_sliver() {
        // 0.01° tall, 40° long.
        let poly = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(40.0)),
            (Degrees(0.01), Degrees(40.0)),
            (Degrees(0.01), Degrees(0.0)),
        ])
        .expect("valid sliver");

        let inside = at(0.005, 20.0);
        let v = offset(&poly, inside);
        assert!(v > 0.0, "sliver interior reported outside");
        assert!(
            v <= Degrees(0.005).radians() + 1e-9,
            "offset {v} exceeds the sliver half-width"
        );

        let outside = at(0.02, 20.0);
        assert!(offset(&poly, outside) < 0.0, "above the sliver, so outside");
    }

    #[test]
    fn test_concave_notch() {
        // An L: the notch is the missing top-right quadrant.
        let poly = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(20.0)),
            (Degrees(10.0), Degrees(20.0)),
            (Degrees(10.0), Degrees(10.0)),
            (Degrees(20.0), Degrees(10.0)),
            (Degrees(20.0), Degrees(0.0)),
        ])
        .expect("valid L");

        // In the notch — inside the bounding box, outside the polygon.
        assert!(offset(&poly, at(15.0, 15.0)) < 0.0);
        // In each arm.
        assert!(offset(&poly, at(5.0, 15.0)) > 0.0);
        assert!(offset(&poly, at(15.0, 5.0)) > 0.0);
    }

    #[test]
    fn test_self_intersecting_star_fill_rules() {
        // A pentagram: joining every second vertex of a pentagon makes the
        // ring cross itself, winding the central pentagon twice and each of
        // the five points once.
        let star: Vec<_> = (0..5)
            .map(|i| {
                let theta = (90.0 + 144.0 * i as f64).to_radians();
                (
                    Degrees(20.0 * f64::sin(theta)),
                    Degrees(20.0 * f64::cos(theta)),
                )
            })
            .collect();
        let nonzero = Polygon::new(star.clone()).expect("valid star");
        let evenodd = Polygon::new(star)
            .expect("valid star")
            .with_fill_rule(FillRule::EvenOdd);

        // Winding 2 — the only place the two rules may disagree.
        let centre = at(0.0, 0.0);
        assert!(offset(&nonzero, centre) > 0.0, "NonZero fills the centre");
        assert!(offset(&evenodd, centre) < 0.0, "EvenOdd leaves a hole");

        // Winding 1, in the top point of the star: both rules say inside.
        let tip = at(17.0, 0.0);
        assert!(offset(&nonzero, tip) > 0.0, "NonZero fills the points");
        assert!(offset(&evenodd, tip) > 0.0, "EvenOdd fills the points too");

        // Winding 0: the rules must agree, values included.
        for p in sphere_points(300) {
            let (a, b) = (offset(&nonzero, p), offset(&evenodd, p));
            if a < 0.0 && b < 0.0 {
                assert!((a - b).abs() < 1e-12, "outside values disagree at {p:?}");
            }
        }
    }

    #[test]
    fn test_polygon_spanning_the_antimeridian() {
        let poly = Polygon::new([
            (Degrees(-5.0), Degrees(175.0)),
            (Degrees(-5.0), Degrees(-175.0)),
            (Degrees(5.0), Degrees(-175.0)),
            (Degrees(5.0), Degrees(175.0)),
        ])
        .expect("valid box");

        for lon in [175.5, 179.0, 180.0, -179.0, -175.5] {
            assert!(
                offset(&poly, at(0.0, lon)) > 0.0,
                "lon {lon} should be inside"
            );
        }
        for lon in [170.0, -170.0, 0.0] {
            assert!(
                offset(&poly, at(0.0, lon)) < 0.0,
                "lon {lon} should be outside"
            );
        }
    }

    #[test]
    fn test_polygon_containing_a_pole() {
        // A ring at 80°N. Great-circle edges bulge poleward between vertices,
        // so densify enough that the ring stays near the parallel.
        let poly = Polygon::new((0..36).map(|i| (Degrees(80.0), Degrees(i as f64 * 10.0 - 180.0))))
            .expect("valid cap");

        assert!(
            offset(&poly, [0.0, 0.0, 1.0]) > 0.0,
            "the north pole is inside a cap around it"
        );
        for lon in [-180.0, -90.0, 0.0, 90.0, 179.0] {
            assert!(
                offset(&poly, at(85.0, lon)) > 0.0,
                "85°N at lon {lon} should be inside"
            );
            assert!(
                offset(&poly, at(70.0, lon)) < 0.0,
                "70°N at lon {lon} should be outside"
            );
        }
        assert!(
            offset(&poly, [0.0, 0.0, -1.0]) < 0.0,
            "the south pole must not read as inside"
        );
    }

    // --- construction ---

    #[test]
    fn test_ring_closes_implicitly() {
        let open = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
        ])
        .expect("valid triangle");
        let closed = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
            (Degrees(0.0), Degrees(0.0)),
        ])
        .expect("explicit closing vertex is accepted");

        assert_eq!(open.verts.len(), closed.verts.len());
        for p in sphere_points(200) {
            assert!((offset(&open, p) - offset(&closed, p)).abs() < 1e-12);
        }
    }

    #[test]
    fn test_consecutive_duplicates_are_dropped() {
        let poly = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
        ])
        .expect("duplicates are dropped, leaving a triangle");
        assert_eq!(poly.verts.len(), 3);
    }

    #[test]
    fn test_too_few_vertices() {
        let two = Polygon::new([(Degrees(0.0), Degrees(0.0)), (Degrees(1.0), Degrees(1.0))]);
        assert_eq!(two, Err(Error::Aoi(super::Error::TooFewVertices(2))));

        // Three vertices that collapse to one.
        let collapsed = Polygon::new([(Degrees(5.0), Degrees(5.0)); 3]);
        assert_eq!(collapsed, Err(Error::Aoi(super::Error::TooFewVertices(1))));
    }

    #[test]
    fn test_latitude_out_of_range() {
        let bad = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(91.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
        ]);
        // The reported latitude is the offending one, not just any.
        assert_eq!(bad, Err(Error::Aoi(super::Error::Latitude(91.0))));
    }

    /// A non-finite latitude fails the range test; a non-finite longitude has
    /// no range to fail, so it needs its own check.
    #[test]
    fn test_non_finite_coordinates_rejected() {
        for lon in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let bad = Polygon::new([
                (Degrees(0.0), Degrees(0.0)),
                (Degrees(10.0), Degrees(lon)),
                (Degrees(10.0), Degrees(10.0)),
            ]);
            assert!(
                matches!(bad, Err(Error::Aoi(super::Error::NotFinite { .. }))),
                "longitude {lon} was not rejected"
            );
        }

        for lat in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let bad = Polygon::new([
                (Degrees(0.0), Degrees(0.0)),
                (Degrees(lat), Degrees(10.0)),
                (Degrees(10.0), Degrees(10.0)),
            ]);
            assert!(
                matches!(bad, Err(Error::Aoi(super::Error::Latitude(_)))),
                "latitude {lat} was not rejected"
            );
        }
    }

    #[test]
    fn test_antipodal_edge_rejected() {
        let bad = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(180.0)),
            (Degrees(45.0), Degrees(90.0)),
        ]);
        assert!(matches!(
            bad,
            Err(Error::Aoi(super::Error::AntipodalEdge { .. }))
        ));
    }

    #[test]
    fn test_larger_than_hemisphere_rejected() {
        // A lune running pole to pole: no cap smaller than a hemisphere holds
        // both poles, so the region it means is genuinely ambiguous.
        let bad = Polygon::new([
            (Degrees(90.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(-90.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(90.0)),
        ]);
        assert!(matches!(
            bad,
            Err(Error::Aoi(super::Error::LargerThanHemisphere { .. }))
        ));
    }

    /// A ring spanning every longitude is *not* rejected: its centroid is the
    /// nearer pole, so it describes the polar cap on that side. Only the
    /// complementary region is inexpressible.
    #[test]
    fn test_full_longitude_ring_is_a_polar_cap() {
        let poly = Polygon::new((0..36).map(|i| (Degrees(1.0), Degrees(i as f64 * 10.0 - 180.0))))
            .expect("a ring at 1°N is the north polar cap");

        assert!(offset(&poly, [0.0, 0.0, 1.0]) > 0.0, "north pole inside");
        assert!(offset(&poly, [0.0, 0.0, -1.0]) < 0.0, "south pole outside");
        assert!(
            offset(&poly, at(-0.5, 42.0)) < 0.0,
            "just south of the ring is outside"
        );
    }

    #[test]
    fn test_degenerate_collinear_polygon_has_no_interior() {
        // All vertices on one great circle (the equator). Everything is
        // outside, and nothing produces a NaN.
        let poly = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(0.0), Degrees(30.0)),
            (Degrees(0.0), Degrees(60.0)),
        ])
        .expect("collinear vertices still form a ring");

        for p in sphere_points(300) {
            let v = offset(&poly, p);
            assert!(v.is_finite(), "non-finite offset at {p:?}");
            assert!(v <= 0.0, "collinear polygon reported an interior at {p:?}");
        }
    }
}
