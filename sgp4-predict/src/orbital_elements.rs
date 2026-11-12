//! Osculating (Keplerian) and mean orbital elements.

use std::f64::consts::TAU;

use chrono::{DateTime, Duration, Utc};

use crate::{Error, Predictor, Result, TemeState, angle::Radians, frames::WGS84_A};

/// Earth's gravitational parameter (m³/s²) from SGP4's WGS-84 geopotential,
/// so osculating elements are consistent with the propagator.
fn mu() -> f64 {
    // `ke` is in earth-radii^1.5 / min and `ae` in km.
    let g = sgp4::WGS84;
    g.ke * g.ke * g.ae.powi(3) * 1e9 / 3600.0
}

/// Below this, an orbit is treated as circular or equatorial.
const SINGULAR: f64 = 1e-9;

/// Osculating Keplerian elements: the two-body orbit that is tangent to the
/// satellite's state at one instant.
///
/// Angles are in `[0, 2π)`. Two geometries are degenerate, and take the usual
/// convention:
///
/// - **Equatorial** (no ascending node): `raan` is zero and angles are
///   measured from the +X axis of the TEME frame, in the direction of motion.
/// - **Circular** (no perigee): `argument_of_perigee` is zero, so
///   `true_anomaly` is the argument of latitude (the true longitude if the
///   orbit is also equatorial).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeplerianElements {
    /// Semi-major axis, in metres.
    pub semi_major_axis: f64,
    /// Eccentricity, in `[0, 1)`.
    pub eccentricity: f64,
    /// Inclination, in `[0, π]`.
    pub inclination: Radians,
    /// Right ascension of the ascending node, measured in the TEME frame.
    pub raan: Radians,
    /// Argument of perigee.
    pub argument_of_perigee: Radians,
    /// True anomaly.
    pub true_anomaly: Radians,
    /// Mean anomaly.
    pub mean_anomaly: Radians,
    /// Mean motion `√(µ/a³)`, in rad/s.
    pub mean_motion: f64,
    /// Orbital period.
    pub period: Duration,
    /// Perigee height above the WGS-84 equatorial radius, in metres.
    pub perigee_altitude: f64,
    /// Apogee height above the WGS-84 equatorial radius, in metres.
    pub apogee_altitude: f64,
}

/// The mean elements an element set was published with, in SI units and
/// [`Radians`].
///
/// `mean_motion` and `period` follow the Kozai convention used by TLEs and
/// OMM messages, so a semi-major axis derived from them differs by a few
/// kilometres from the Brouwer value SGP4 propagates internally.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanElements {
    /// Element-set epoch.
    pub epoch: DateTime<Utc>,
    /// Inclination.
    pub inclination: Radians,
    /// Right ascension of the ascending node.
    pub raan: Radians,
    /// Eccentricity.
    pub eccentricity: f64,
    /// Argument of perigee.
    pub argument_of_perigee: Radians,
    /// Mean anomaly at `epoch`.
    pub mean_anomaly: Radians,
    /// Mean motion, in rad/s.
    pub mean_motion: f64,
    /// Orbital period.
    pub period: Duration,
    /// SGP4 drag term B*, in inverse earth radii.
    pub bstar: f64,
    /// First derivative of mean motion, as published (rev/day²).
    pub mean_motion_dot: f64,
    /// Second derivative of mean motion, as published (rev/day³).
    pub mean_motion_ddot: f64,
}

pub(crate) type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Wrap into `[0, 2π)`; `rem_euclid` can round a tiny negative to a full turn.
fn wrap(angle: f64) -> Radians {
    let a = angle.rem_euclid(TAU);
    Radians(if a >= TAU { 0.0 } else { a })
}

fn seconds(s: f64) -> Duration {
    Duration::microseconds((s * 1e6).round() as i64)
}

/// The orbit plane's axes: the angular-momentum direction, the ascending node
/// and the in-plane direction 90° ahead of it. With no node (an equatorial
/// orbit) the node axis is TEME +X and `has_node` is false.
pub(crate) struct OrbitPlane {
    pub h_hat: V,
    pub node_hat: V,
    pub ahead: V,
    pub has_node: bool,
}

impl OrbitPlane {
    /// In-plane angle of `u` from the node, along the direction of motion.
    pub fn angle(&self, u: V) -> f64 {
        dot(u, self.ahead).atan2(dot(u, self.node_hat))
    }

    /// `(sin, cos)` of [`angle`](Self::angle), without the `atan2`.
    pub fn sin_cos(&self, u: V) -> (f64, f64) {
        let norm = dot(u, u).sqrt();
        (dot(u, self.ahead) / norm, dot(u, self.node_hat) / norm)
    }
}

impl TemeState {
    pub(crate) fn orbit_plane(&self) -> OrbitPlane {
        let r = [self.position.x, self.position.y, self.position.z];
        let v = [self.velocity.x, self.velocity.y, self.velocity.z];
        let h = cross(r, v);
        let h_hat = scale(h, 1.0 / dot(h, h).sqrt());
        // ẑ × ĥ, whose length is sin(inclination).
        let node = [-h_hat[1], h_hat[0], 0.0];
        let node_norm = node[0].hypot(node[1]);
        let has_node = node_norm >= SINGULAR;
        let node_hat = if has_node {
            scale(node, 1.0 / node_norm)
        } else {
            [1.0, 0.0, 0.0]
        };
        OrbitPlane {
            h_hat,
            node_hat,
            ahead: cross(h_hat, node_hat),
            has_node,
        }
    }

    /// Osculating Keplerian elements of this state.
    ///
    /// # Errors
    /// [`Error::NotBound`] if the state is on a parabolic or hyperbolic orbit.
    pub fn keplerian(&self) -> Result<KeplerianElements> {
        let mu = mu();
        let r = [self.position.x, self.position.y, self.position.z];
        let v = [self.velocity.x, self.velocity.y, self.velocity.z];
        let r_norm = dot(r, r).sqrt();

        let energy = dot(v, v) / 2.0 - mu / r_norm;
        if energy.partial_cmp(&0.0) != Some(std::cmp::Ordering::Less) {
            return Err(Error::NotBound);
        }
        let a = -mu / (2.0 * energy);

        let plane = self.orbit_plane();
        let e_vec = scale(
            [
                (dot(v, v) - mu / r_norm) * r[0] - dot(r, v) * v[0],
                (dot(v, v) - mu / r_norm) * r[1] - dot(r, v) * v[1],
                (dot(v, v) - mu / r_norm) * r[2] - dot(r, v) * v[2],
            ],
            1.0 / mu,
        );
        let e = dot(e_vec, e_vec).sqrt();
        let raan = if plane.has_node {
            plane.node_hat[1].atan2(plane.node_hat[0])
        } else {
            0.0
        };
        let angle = |u: V| plane.angle(u);

        let argp = if e < SINGULAR { 0.0 } else { angle(e_vec) };
        let nu = angle(r) - argp;
        let ecc_anomaly =
            2.0 * ((1.0 - e).sqrt() * (nu / 2.0).sin()).atan2((1.0 + e).sqrt() * (nu / 2.0).cos());
        let n = (mu / a.powi(3)).sqrt();

        Ok(KeplerianElements {
            semi_major_axis: a,
            eccentricity: e,
            inclination: Radians(plane.h_hat[2].clamp(-1.0, 1.0).acos()),
            raan: wrap(raan),
            argument_of_perigee: wrap(argp),
            true_anomaly: wrap(nu),
            mean_anomaly: wrap(ecc_anomaly - e * ecc_anomaly.sin()),
            mean_motion: n,
            period: seconds(TAU / n),
            perigee_altitude: a * (1.0 - e) - WGS84_A,
            apogee_altitude: a * (1.0 + e) - WGS84_A,
        })
    }
}

impl Predictor {
    /// Osculating Keplerian elements at `t`.
    ///
    /// # Errors
    /// Propagation errors; see [`TemeState::keplerian`].
    pub fn osculating_elements(&self, t: DateTime<Utc>) -> Result<KeplerianElements> {
        self.propagate(t)?.keplerian()
    }

    /// Osculating argument of latitude at `t`, in `[0, 360°)`: the angle from
    /// the ascending node to the satellite, measured along the orbit.
    ///
    /// The node is poorly conditioned as the inclination approaches zero, so
    /// the value is noisy for near-equatorial orbits.
    ///
    /// # Errors
    /// Propagation errors, and [`Error::NoAscendingNode`] for an equatorial
    /// orbit.
    pub fn argument_of_latitude(&self, t: DateTime<Utc>) -> Result<Radians> {
        let s = self.propagate(t)?;
        let plane = s.orbit_plane();
        if !plane.has_node {
            return Err(Error::NoAscendingNode);
        }
        Ok(wrap(plane.angle([
            s.position.x,
            s.position.y,
            s.position.z,
        ])))
    }

    /// The mean elements this predictor was built from.
    #[must_use]
    pub fn mean_elements(&self) -> MeanElements {
        let e = &self.elements;
        let rad = |deg: f64| Radians(deg.to_radians());
        MeanElements {
            epoch: self.epoch(),
            inclination: rad(e.inclination),
            raan: rad(e.right_ascension),
            eccentricity: e.eccentricity,
            argument_of_perigee: rad(e.argument_of_perigee),
            mean_anomaly: rad(e.mean_anomaly),
            mean_motion: e.mean_motion * TAU / 86_400.0,
            period: seconds(86_400.0 / e.mean_motion),
            bstar: e.drag_term,
            mean_motion_dot: e.mean_motion_dot,
            mean_motion_ddot: e.mean_motion_ddot,
        }
    }
}
