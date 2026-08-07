//! Area-of-interest detection: when is the satellite's ground track inside a
//! region on Earth's surface?
//!
//! [`AoiIter`] yields the [`AoiWindow`]s during which the satellite's ground
//! point lies inside an [`Area`]. It is a thin wrapper over the generic
//! [`WindowIter`](crate::WindowIter), like [`TransitIter`](crate::TransitIter):
//! the event function is a signed angular offset from the area's boundary, and
//! the windows are where it is positive.
//!
//! [`Polygon`] is the general shape — an arbitrary ring of latitude/longitude
//! vertices, which may be concave or self-intersecting. [`Rectangle`] is a
//! plain latitude/longitude box, and [`Ellipse`] covers circular footprints
//! and oriented elliptical ones. Implement [`Area`] on your own type for
//! shapes this crate does not provide.
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

/// A region on Earth's surface that a ground track can pass over.
///
/// Implemented here by [`Polygon`], [`Rectangle`] and [`Ellipse`]. Implement
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
    fn signed_angular_offset(&self, point: LatLon) -> Radians;
}

impl<A: Area + ?Sized> Area for &A {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        (**self).signed_angular_offset(point)
    }
}

/// How the interior of a self-intersecting [`Polygon`] is determined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
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
    pub fn with_fill_rule(mut self, fill: FillRule) -> Self {
        self.fill = fill;
        self
    }

    /// The polygon's vertices in ring order, after deduplication.
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

        // Outside the bounding cap. This branch is a correctness gate as much
        // as a fast path: `winding` is only valid once the antipode is known
        // to be outside the region, which the cap guarantees. The result is a
        // lower bound on the true distance, which is all `Area` promises, and
        // it is strictly negative so it can never be mistaken for "inside".
        let from_axis = angle_between(self.cap_axis, p);
        if from_axis > self.cap_radius + COINCIDENT {
            return Radians(-(from_axis - self.cap_radius));
        }

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
#[derive(Debug, Clone)]
pub struct Rectangle {
    south: f64,
    north: f64,
    west: f64,
    /// Longitude extent eastward from `west`, in `(0, 2π]`.
    lon_span: f64,
    /// `None` when the box spans every longitude, so it has no side edges.
    sides: Option<Sides>,
}

#[derive(Debug, Clone)]
struct Sides {
    corners: [[f64; 3]; 4],
    meridians: [Meridian; 2],
}

#[derive(Debug, Clone, Copy)]
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
    pub fn latitudes(&self) -> (Degrees, Degrees) {
        (
            Radians(self.south).to_degrees(),
            Radians(self.north).to_degrees(),
        )
    }

    /// The western bound and the extent eastward from it. The extent is 360°
    /// for a box built by [`latitude_band`](Rectangle::latitude_band).
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

/// An ellipse on Earth's surface.
///
/// The set of points whose great-circle distances to two foci sum to at most
/// twice the semi-major axis — the spherical reading of the planar definition.
/// A [`circle`](Ellipse::circle) is the case where the two foci coincide.
///
/// Semi-axes are **angular**, like every other measurement here. A degree of
/// arc is about 111.2 km on the ground, so a 300 km semi-major axis is roughly
/// `Degrees(2.7)`.
///
/// # Examples
///
/// ```
/// use sgp4_predict::{Degrees, Ellipse, LatLon};
///
/// // Roughly 300 km by 120 km, major axis pointing north-east.
/// let north_sea = Ellipse::new(
///     LatLon { latitude: Degrees(56.0), longitude: Degrees(2.0) },
///     Degrees(2.7),
///     Degrees(1.1),
///     Degrees(45.0),
/// )?;
///
/// // A circular area 500 km across.
/// let cape_town = Ellipse::circle((Degrees(-33.9), Degrees(18.4)), Degrees(2.25))?;
/// # Ok::<(), sgp4_predict::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Ellipse {
    centre: [f64; 3],
    /// Both equal to `centre` when the ellipse is a circle.
    foci: [[f64; 3]; 2],
    semi_major: f64,
    semi_minor: f64,
    /// Normalized into `[0, 2π)`.
    bearing: f64,
}

impl Ellipse {
    /// Build an ellipse from its centre, semi-axes, and the bearing of its
    /// major axis — degrees clockwise from north, so `0` aims the major axis
    /// at the pole and `90` aims it east.
    ///
    /// At a pole, where north is undefined, the bearing is measured from the
    /// direction of the prime meridian instead.
    ///
    /// # Errors
    ///
    /// - [`Error::Latitude`] if the centre's latitude is outside `[-90, 90]`.
    /// - [`Error::NotFinite`] if the centre's longitude, either semi-axis, or
    ///   the bearing is NaN or infinite. Longitude and bearing are themselves
    ///   unbounded — they wrap — so only finiteness is checked.
    /// - [`Error::EllipseAxes`] unless `0 < semi_minor <= semi_major < 90°`.
    pub fn new(
        centre: impl Into<LatLon>,
        semi_major: Degrees,
        semi_minor: Degrees,
        bearing: Degrees,
    ) -> Result<Self> {
        let centre = centre.into();
        checked_latitude(centre.latitude)?;
        checked_angle(centre.longitude, "ellipse centre longitude")?;
        let bearing_rad = checked_angle(bearing, "ellipse bearing")?;

        let (a, b) = (
            checked_angle(semi_major, "ellipse semi-major axis")?,
            checked_angle(semi_minor, "ellipse semi-minor axis")?,
        );
        if !(b > 0.0 && b <= a + COINCIDENT && a < FRAC_PI_2 - COINCIDENT) {
            return Err(Error::EllipseAxes {
                semi_major_deg: semi_major.to_f64(),
                semi_minor_deg: semi_minor.to_f64(),
            }
            .into());
        }
        let b = b.min(a);

        // Half the focal separation, from the spherical right triangle joining
        // the centre, one focus and a minor-axis endpoint: `cos a = cos b cos c`.
        // The endpoint is `a` from each focus, since the two distances there
        // are equal and sum to `2a`.
        let c = (a.cos() / b.cos()).clamp(-1.0, 1.0).acos();

        let centre = unit_from_lat_lon(centre);
        let (north, east) = local_frame(centre);
        let (sin_brg, cos_brg) = bearing_rad.sin_cos();
        let major = [
            north[0] * cos_brg + east[0] * sin_brg,
            north[1] * cos_brg + east[1] * sin_brg,
            north[2] * cos_brg + east[2] * sin_brg,
        ];
        let (sin_c, cos_c) = c.sin_cos();
        let focus = |sign: f64| {
            [
                centre[0] * cos_c + sign * major[0] * sin_c,
                centre[1] * cos_c + sign * major[1] * sin_c,
                centre[2] * cos_c + sign * major[2] * sin_c,
            ]
        };

        Ok(Self {
            centre,
            foci: [focus(1.0), focus(-1.0)],
            semi_major: a,
            semi_minor: b,
            bearing: bearing.normalized().radians(),
        })
    }

    /// Build a circular area of angular `radius` — a spherical cap.
    ///
    /// # Errors
    ///
    /// As [`Ellipse::new`]: the radius must be positive and under 90°.
    pub fn circle(centre: impl Into<LatLon>, radius: Degrees) -> Result<Self> {
        Self::new(centre, radius, radius, Degrees(0.0))
    }

    /// The ellipse's centre.
    pub fn centre(&self) -> LatLon {
        lat_lon_from_unit(self.centre)
    }

    /// The semi-major and semi-minor axes, as angles.
    pub fn semi_axes(&self) -> (Degrees, Degrees) {
        (
            Radians(self.semi_major).to_degrees(),
            Radians(self.semi_minor).to_degrees(),
        )
    }

    /// Bearing of the major axis, degrees clockwise from north.
    pub fn bearing(&self) -> Degrees {
        Radians(self.bearing).to_degrees()
    }

    /// The two foci, which coincide with the centre when the ellipse is a
    /// circle.
    pub fn foci(&self) -> (LatLon, LatLon) {
        (
            lat_lon_from_unit(self.foci[0]),
            lat_lon_from_unit(self.foci[1]),
        )
    }
}

impl Area for Ellipse {
    fn signed_angular_offset(&self, point: LatLon) -> Radians {
        let p = unit_from_lat_lon(point);
        let sum = angle_between(self.foci[0], p) + angle_between(self.foci[1], p);

        // Each distance to a focus is 1-Lipschitz along the surface, so their
        // sum is 2-Lipschitz and half the shortfall from `2a` can never exceed
        // the distance to the boundary — an under-estimate for an eccentric
        // ellipse, exact for a circle, where the two terms coincide.
        let d = self.semi_major - sum / 2.0;
        if d.abs() < ON_BOUNDARY {
            return Radians(0.0);
        }
        Radians(d)
    }
}

/// The window during which the satellite's ground track lies inside an
/// [`Area`].
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators to cover a specific
/// overpass, and [`TimeWindow`](crate::TimeWindow) for
/// [`clamp`](crate::TimeWindow::clamp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AoiWindow {
    /// When the ground track crosses into the area.
    pub start: DateTime<Utc>,
    /// When it crosses back out.
    pub end: DateTime<Utc>,
}

impl AoiWindow {
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

/// Event function: the ground track's signed angular offset from the area's
/// boundary. Positive inside, so the windows where it is positive are the
/// overpasses.
///
/// No rate is supplied: the offset is not differentiable across the medial
/// axis or a vertex bisector, which is exactly the geometry that matters here,
/// and the bracketed solver converges on the time bracket regardless.
pub(crate) struct GroundTrackInside<'a, A: Area> {
    predictor: Predictor,
    area: &'a A,
}

impl<'a, A: Area> EventFunction for GroundTrackInside<'a, A> {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        let point = self.predictor.sub_point(t)?;
        Ok(Sample {
            time: t,
            value: self.area.signed_angular_offset(point.into()).to_f64(),
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
/// The `min` floor is the one exception, and the reason it exists: without it
/// the step collapses to zero at the boundary and the scan stalls. A chord the
/// ground track traverses in less than `min` can still be missed.
#[derive(Debug, Clone, Copy)]
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
///   contribution of radial motion to geodetic latitude.
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
#[derive(Debug, Clone, Copy)]
pub struct AoiIterOpts {
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

/// Iterator over the windows during which the satellite's ground track lies
/// inside an area.
///
/// Created by [`Predictor::aoi_iter`](crate::Predictor::aoi_iter).
pub struct AoiIter<'a, A: Area> {
    inner: WindowIter<GroundTrackInside<'a, A>, ProximityStep>,
}

impl<'a, A: Area> AoiIter<'a, A> {
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
            .event_function(GroundTrackInside { predictor, area })
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
    /// Find every window in which the satellite's ground track lies inside
    /// `area`.
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

    /// Detect whether the ground track is inside `area` at time `t`.
    ///
    /// Returns `Ok(None)` if it is outside. Otherwise walks backward and
    /// forward from `t` using [`AoiIterOpts::default`]'s `walk_step` to find
    /// the entry and exit crossings, refining each with the bracketed hybrid
    /// solver ([`Refinement`]).
    ///
    /// Returns [`Error::Detect`](crate::Error::Detect) if the window is longer
    /// than [`AoiIterOpts::default`]'s `max_window_duration`.
    pub fn detect_aoi<A: Area>(&self, t: DateTime<Utc>, area: &A) -> Result<Option<AoiWindow>> {
        self.detect_aoi_with_opts(t, area, AoiIterOpts::default())
    }

    /// Like [`Predictor::detect_aoi`], but with a customized walk step and max
    /// window duration. Only [`AoiIterOpts::walk_step`] and
    /// [`AoiIterOpts::max_window_duration`] are used — the other fields don't
    /// apply to this single-point detection.
    pub fn detect_aoi_with_opts<A: Area>(
        &self,
        t: DateTime<Utc>,
        area: &A,
        opts: AoiIterOpts,
    ) -> Result<Option<AoiWindow>> {
        let mut f = GroundTrackInside {
            predictor: self.clone(),
            area,
        };
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
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("polygon needs at least 3 distinct vertices, got {0}")]
    TooFewVertices(usize),
    #[error("latitude {0} is outside [-90, 90]")]
    Latitude(f64),
    #[error("{what} is not finite: {value}")]
    NotFinite { what: &'static str, value: f64 },
    #[error("polygon edge {index} joins antipodal vertices; no unique great-circle arc joins them")]
    AntipodalEdge { index: usize },
    #[error(
        "polygon spans {radius_deg:.1}° from its centre and does not fit within a hemisphere; \
         split it into smaller polygons, or describe the complementary region instead"
    )]
    LargerThanHemisphere { radius_deg: f64 },
    #[error(
        "rectangle is empty: south {south}° must lie below north {north}°, and the corners \
         must differ in longitude — note that -180° and 180° are the same meridian, so use \
         `Rectangle::latitude_band` for a box spanning every longitude"
    )]
    EmptyRectangle { south: f64, north: f64 },
    #[error(
        "ellipse semi-axes must satisfy 0 < semi-minor ({semi_minor_deg}°) <= semi-major \
         ({semi_major_deg}°) < 90°"
    )]
    EllipseAxes {
        semi_major_deg: f64,
        semi_minor_deg: f64,
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
/// exact; only the *magnitude* of the offset is distorted relative to true
/// ellipsoidal distance, and [`Area`] does not promise that magnitude.
fn unit_from_lat_lon(p: LatLon) -> [f64; 3] {
    let (sin_lat, cos_lat) = p.latitude.radians().sin_cos();
    let (sin_lon, cos_lon) = p.longitude.radians().sin_cos();
    [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat]
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

/// North and east unit vectors at `p`. At a pole, where north is undefined,
/// "north" points along the prime meridian instead.
fn local_frame(p: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let north = normalize(reject([0.0, 0.0, 1.0], p))
        .or_else(|| normalize(reject([1.0, 0.0, 0.0], p)))
        .expect("p cannot be parallel to both axes");
    (north, cross(north, p))
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
mod ellipse_tests {
    use super::*;
    use crate::Error;

    /// Roughly 300 km by 120 km over the North Sea, major axis north-east.
    fn north_sea() -> Ellipse {
        Ellipse::new(
            (Degrees(56.0), Degrees(2.0)),
            Degrees(2.7),
            Degrees(1.1),
            Degrees(45.0),
        )
        .expect("valid ellipse")
    }

    /// The point `distance` away from `centre` along `bearing`, degrees
    /// clockwise from north.
    fn destination(centre: LatLon, distance: Degrees, bearing: Degrees) -> LatLon {
        let c = unit_from_lat_lon(centre);
        let (north, east) = local_frame(c);
        let (sin_b, cos_b) = bearing.radians().sin_cos();
        let (sin_d, cos_d) = distance.radians().sin_cos();
        lat_lon_from_unit(
            [0, 1, 2].map(|i| c[i] * cos_d + (north[i] * cos_b + east[i] * sin_b) * sin_d),
        )
    }

    fn offset(e: &Ellipse, p: LatLon) -> f64 {
        e.signed_angular_offset(p).to_f64()
    }

    /// A circle is the one case where the offset is the exact signed distance,
    /// not merely a lower bound.
    #[test]
    fn test_circle_offset_is_the_exact_signed_distance() {
        let centre = LatLon::new(Degrees(-33.9), Degrees(18.4));
        let radius = Degrees(2.25);
        let circle = Ellipse::circle(centre, radius).expect("valid circle");

        let (f1, f2) = circle.foci();
        assert!(coincident(unit_from_lat_lon(f1), unit_from_lat_lon(f2)));

        let c = unit_from_lat_lon(centre);
        for p in super::geometry_tests::sphere_points(500) {
            let truth = radius.radians() - angle_between(c, p);
            assert!(
                (offset(&circle, lat_lon_from_unit(p)) - truth).abs() < 1e-12,
                "circle offset should equal {truth} at {:?}",
                lat_lon_from_unit(p)
            );
        }
    }

    /// The four axis endpoints define the ellipse, so all four must read as
    /// exactly on the boundary.
    #[test]
    fn test_axis_endpoints_are_on_the_boundary() {
        let e = north_sea();
        let centre = e.centre();
        let (a, b) = e.semi_axes();
        let brg = e.bearing().to_f64();

        for (distance, bearing) in [
            (a, brg),
            (a, brg + 180.0),
            (b, brg + 90.0),
            (b, brg + 270.0),
        ] {
            let p = destination(centre, distance, Degrees(bearing));
            assert!(
                offset(&e, p).abs() < 1e-9,
                "{distance:?} at bearing {bearing}° should be on the boundary, got {}",
                offset(&e, p)
            );
        }
        assert!(offset(&e, centre) > 0.0, "the centre must be inside");
    }

    /// Bearing orients the major axis: the ellipse reaches further along it
    /// than across it.
    #[test]
    fn test_bearing_orients_the_major_axis() {
        let e = north_sea();
        let centre = e.centre();
        // Between the two semi-axes, so inside along the major axis and
        // outside across it.
        let between = Degrees(1.9);

        assert!(offset(&e, destination(centre, between, Degrees(45.0))) > 0.0);
        assert!(offset(&e, destination(centre, between, Degrees(225.0))) > 0.0);
        assert!(offset(&e, destination(centre, between, Degrees(135.0))) < 0.0);
        assert!(offset(&e, destination(centre, between, Degrees(315.0))) < 0.0);
    }

    #[test]
    fn test_offset_never_exceeds_true_distance() {
        let e = north_sea();
        let centre = e.centre();
        let (a, b) = e.semi_axes();

        // Boundary sample by bisecting the radius at each bearing: the offset
        // is monotone in distance from the centre, so the crossing is unique.
        let mut boundary = Vec::new();
        for i in 0..2_000 {
            let bearing = Degrees(360.0 * i as f64 / 2_000.0);
            let (mut lo, mut hi) = (0.0, a.to_f64() + 1e-9);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if offset(&e, destination(centre, Degrees(mid), bearing)) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            boundary.push(unit_from_lat_lon(destination(
                centre,
                Degrees(0.5 * (lo + hi)),
                bearing,
            )));
            assert!(
                0.5 * (lo + hi) >= b.to_f64() - 1e-9,
                "no boundary point may lie inside the semi-minor axis"
            );
        }

        for p in super::geometry_tests::sphere_points(500) {
            let ll = lat_lon_from_unit(p);
            let reported = offset(&e, ll).abs();
            let truth = boundary
                .iter()
                .map(|&q| angle_between(p, q))
                .fold(f64::INFINITY, f64::min);
            assert!(
                reported <= truth + 1e-6,
                "reported {reported} exceeds true distance {truth} at {ll:?}"
            );
        }
    }

    #[test]
    fn test_far_side_of_the_earth_is_far() {
        let e = north_sea();
        let antipode = LatLon::new(Degrees(-56.0), Degrees(-178.0));
        assert!(offset(&e, antipode) < -3.0);
        for (lat, lon) in [(0.0, 0.0), (56.0, 90.0), (-40.0, 2.0), (89.0, 2.0)] {
            let v = offset(&e, LatLon::new(Degrees(lat), Degrees(lon)));
            assert!(v < 0.0, "({lat}, {lon}) should be outside, got {v}");
        }
    }

    /// North is undefined at a pole, so the bearing falls back to the prime
    /// meridian. The geometry must still be well formed.
    #[test]
    fn test_pole_centred_ellipse() {
        let e = Ellipse::new(
            (Degrees(90.0), Degrees(0.0)),
            Degrees(10.0),
            Degrees(4.0),
            Degrees(0.0),
        )
        .expect("valid ellipse");

        assert!(offset(&e, LatLon::new(Degrees(90.0), Degrees(0.0))) > 0.0);
        // The major axis runs down the prime meridian and its antimeridian.
        assert!(offset(&e, LatLon::new(Degrees(81.0), Degrees(0.0))) > 0.0);
        assert!(offset(&e, LatLon::new(Degrees(81.0), Degrees(180.0))) > 0.0);
        // The minor axis, a quarter turn away, falls short.
        assert!(offset(&e, LatLon::new(Degrees(81.0), Degrees(90.0))) < 0.0);
        assert!(offset(&e, LatLon::new(Degrees(81.0), Degrees(-90.0))) < 0.0);
    }

    #[test]
    fn test_invalid_axes_rejected() {
        let centre = (Degrees(56.0), Degrees(2.0));
        for (major, minor) in [
            (1.0, 2.0),  // minor exceeds major
            (1.0, 0.0),  // degenerate
            (1.0, -1.0), // negative
            (90.0, 1.0), // a hemisphere across
            (120.0, 1.0),
        ] {
            let err = Ellipse::new(centre, Degrees(major), Degrees(minor), Degrees(0.0))
                .expect_err("should be rejected");
            assert!(
                matches!(err, Error::Aoi(super::Error::EllipseAxes { .. })),
                "{major}/{minor} gave {err}"
            );
        }
        assert!(matches!(
            Ellipse::circle((Degrees(91.0), Degrees(0.0)), Degrees(1.0)).expect_err("bad latitude"),
            Error::Aoi(super::Error::Latitude(_))
        ));
    }

    /// The latitude range test rejects a non-finite latitude on its own; the
    /// longitude, the bearing and the axes have no range that does. Unchecked,
    /// a NaN is built into the foci and every offset is NaN, which
    /// `ProximityStep` floors to `min_step` — the whole interval scanned at a
    /// millisecond, with no error ever surfacing.
    #[test]
    fn test_non_finite_arguments_rejected() {
        let centre = (Degrees(56.0), Degrees(2.0));
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for (label, err) in [
                (
                    "centre longitude",
                    Ellipse::new(
                        (Degrees(56.0), Degrees(bad)),
                        Degrees(2.7),
                        Degrees(1.1),
                        Degrees(45.0),
                    ),
                ),
                (
                    "bearing",
                    Ellipse::new(centre, Degrees(2.7), Degrees(1.1), Degrees(bad)),
                ),
                (
                    "semi-major",
                    Ellipse::new(centre, Degrees(bad), Degrees(1.1), Degrees(45.0)),
                ),
                (
                    "semi-minor",
                    Ellipse::new(centre, Degrees(2.7), Degrees(bad), Degrees(45.0)),
                ),
                ("radius", Ellipse::circle(centre, Degrees(bad))),
            ] {
                assert!(
                    matches!(err, Err(Error::Aoi(super::Error::NotFinite { .. }))),
                    "{label} {bad} was not rejected"
                );
            }
            assert!(
                matches!(
                    Ellipse::circle((Degrees(bad), Degrees(2.0)), Degrees(1.0)),
                    Err(Error::Aoi(super::Error::Latitude(_)))
                ),
                "centre latitude {bad} was not rejected"
            );
        }
    }

    #[test]
    fn test_accessors_round_trip() {
        let e = north_sea();
        let centre = e.centre();
        assert!((centre.latitude.to_f64() - 56.0).abs() < 1e-12);
        assert!((centre.longitude.to_f64() - 2.0).abs() < 1e-12);
        let (a, b) = e.semi_axes();
        assert!((a.to_f64() - 2.7).abs() < 1e-12);
        assert!((b.to_f64() - 1.1).abs() < 1e-12);
        assert!((e.bearing().to_f64() - 45.0).abs() < 1e-12);

        // A bearing outside [0, 360) reads back normalized.
        let wrapped = Ellipse::new(
            (Degrees(0.0), Degrees(0.0)),
            Degrees(2.0),
            Degrees(1.0),
            Degrees(-90.0),
        )
        .expect("valid ellipse");
        assert!((wrapped.bearing().to_f64() - 270.0).abs() < 1e-12);
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
        assert!(matches!(
            two,
            Err(Error::Aoi(super::Error::TooFewVertices(2)))
        ));

        // Three vertices that collapse to one.
        let collapsed = Polygon::new([(Degrees(5.0), Degrees(5.0)); 3]);
        assert!(matches!(
            collapsed,
            Err(Error::Aoi(super::Error::TooFewVertices(1)))
        ));
    }

    #[test]
    fn test_latitude_out_of_range() {
        let bad = Polygon::new([
            (Degrees(0.0), Degrees(0.0)),
            (Degrees(91.0), Degrees(10.0)),
            (Degrees(10.0), Degrees(10.0)),
        ]);
        assert!(matches!(bad, Err(Error::Aoi(super::Error::Latitude(_)))));
    }

    /// A non-finite latitude fails the range test; a non-finite longitude has
    /// no range to fail, so it needs its own check. Both must be an error
    /// rather than the panic a NaN vertex used to reach in `normalize`.
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
