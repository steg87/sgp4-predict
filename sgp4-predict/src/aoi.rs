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
//! vertices, which may be concave or self-intersecting. Implement [`Area`] on
//! your own type for shapes this crate does not provide.
//!
//! An [`AoiWindow`] implements [`IntervalRange`], so it can be passed directly
//! to [`Predictor::prediction_iter`] or [`Predictor::observation_iter`] to
//! iterate over a specific overpass.
//!
//! # Geometry
//!
//! Polygon edges are **great-circle arcs**, in the sphere obtained by treating
//! geodetic latitude as spherical latitude — the same convention as S2,
//! BigQuery GIS, and GeoJSON-on-a-sphere. They are neither rhumb lines nor
//! lines of constant latitude, so four vertices at 60°N do not trace the 60°N
//! parallel: the arcs between them bulge to roughly 68°N. Densify long edges.
//!
//! [`IntervalRange`]: crate::IntervalRange
//! [`Predictor::prediction_iter`]: crate::Predictor::prediction_iter
//! [`Predictor::observation_iter`]: crate::Predictor::observation_iter

use std::f64::consts::{FRAC_PI_2, TAU};

use chrono::{DateTime, Duration, Utc};
use sgp4::Elements;
use thiserror::Error as ThisError;

use crate::{
    Predictor, Result,
    angle::Radians,
    detect::{self, EventFunction, MIN_POSITIVE_STEP, Sample, StepStrategy, WindowIter},
    frames::{LatLon, WGS84_E2},
    roots::Refinement,
    time::{self, IntervalRange},
};

/// Vertices closer together than this are treated as duplicates.
const COINCIDENT: f64 = 1e-9;

/// A ground point within this angle of the boundary is reported as exactly on
/// it. Roughly 6 nanometres of arc.
const ON_BOUNDARY: f64 = 1e-15;

/// A region on Earth's surface that a ground track can pass over.
///
/// Implemented here by [`Polygon`]. Implement it on your own type to detect
/// windows over a shape this crate does not provide.
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

/// The window during which the satellite's ground track lies inside an
/// [`Area`].
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators to cover a specific
/// overpass.
#[derive(Debug, Clone, Copy)]
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

    /// Returns a copy of this window clamped to `interval`, or `None` if it
    /// lies entirely outside.
    pub fn clamp(&self, interval: &impl time::IntervalRange) -> Option<AoiWindow> {
        self.intersection(interval)
            .map(|r| AoiWindow::new(r.start, r.end))
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
                // Clamping the f64 seconds before the Duration conversion
                // keeps it from overflowing and already bounds the result to
                // self.max; only the floor still needs enforcing. It also
                // funnels a NaN value into `min` rather than a stall.
                let seconds =
                    (s.value.abs() / self.angular_rate).clamp(0.0, self.max.num_seconds() as f64);
                Duration::milliseconds((seconds * 1e3) as i64).max(self.min)
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
    /// crossing the scan is guaranteed to see.
    pub min_step: Duration,
    /// Upper bound of the adaptive coarse-scan step, used when the ground
    /// track is far from the area.
    pub max_step: Duration,
    /// Fixed step used to walk from a window's start to its end.
    ///
    /// Unlike the coarse scan this has no skip guarantee, so for a **concave**
    /// area a notch the ground track leaves and re-enters within `walk_step`
    /// is absorbed into the surrounding window. A convex area is unaffected.
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
        let step = ProximityStep {
            min: opts.min_step.max(MIN_POSITIVE_STEP),
            max: opts.max_step.max(MIN_POSITIVE_STEP),
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
    #[error("polygon edge {index} joins antipodal vertices; no unique great-circle arc joins them")]
    AntipodalEdge { index: usize },
    #[error(
        "polygon spans {radius_deg:.1}° from its centre and does not fit within a hemisphere; \
         split it into smaller polygons, or describe the complementary region instead"
    )]
    LargerThanHemisphere { radius_deg: f64 },
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

fn lat_lon_from_unit(v: [f64; 3]) -> LatLon {
    LatLon {
        latitude: Radians(v[2].clamp(-1.0, 1.0).asin()).to_degrees(),
        longitude: Radians(v[1].atan2(v[0])).to_degrees(),
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use chrono::TimeZone;

    fn step(value: Option<f64>) -> Duration {
        let now = Utc.with_ymd_and_hms(2025, 12, 20, 12, 0, 0).unwrap();
        let mut s = ProximityStep {
            min: Duration::seconds(1),
            max: Duration::minutes(10),
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

    // --- max_sub_point_rate ---

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

    /// Deterministic uniform points on the sphere, so failures reproduce.
    fn sphere_points(n: usize) -> Vec<[f64; 3]> {
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
            let boundary = boundary(&poly, 4000);
            for p in sphere_points(500) {
                let reported = offset(&poly, p).abs();
                let truth = boundary
                    .iter()
                    .map(|&b| angle_between(p, b))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    reported <= truth + 1e-6,
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
