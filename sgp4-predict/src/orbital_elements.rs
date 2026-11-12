//! Osculating (Keplerian) and mean orbital elements.

use std::f64::consts::TAU;

use chrono::{DateTime, Duration, Utc};

use crate::{
    Error, Predictor, Result, TemeState,
    angle::Radians,
    frames::WGS84_A,
    vectors::{Axis, StateVector, UnitVector, Vec3},
};

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

fn seconds(s: f64) -> Duration {
    Duration::microseconds((s * 1e6).round() as i64)
}

/// The orbit plane's axes: the angular-momentum direction, the ascending node
/// and the in-plane direction 90° ahead of it. With no node (an equatorial
/// orbit) the node axis is +X and `has_node` is false.
pub(crate) struct OrbitPlane<F> {
    pub h_hat: UnitVector<F>,
    pub node_hat: UnitVector<F>,
    pub ahead: UnitVector<F>,
    pub has_node: bool,
}

impl<F> OrbitPlane<F> {
    /// In-plane angle of `u` from the node, along the direction of motion.
    pub fn angle<K>(&self, u: &Vec3<K, F>) -> f64 {
        u.dot(&self.ahead).atan2(u.dot(&self.node_hat))
    }

    /// `(sin, cos)` of [`angle`](Self::angle), without the `atan2`.
    pub fn sin_cos<K>(&self, u: &Vec3<K, F>) -> (f64, f64) {
        let norm = u.norm();
        (u.dot(&self.ahead) / norm, u.dot(&self.node_hat) / norm)
    }
}

impl<F> StateVector<F> {
    pub(crate) fn orbit_plane(&self) -> OrbitPlane<F> {
        let h_hat = self.position.cross(&self.velocity).unit();
        // ẑ × ĥ, whose length is sin(inclination).
        let node = UnitVector::new(-h_hat.y, h_hat.x, 0.0);
        let has_node = node.norm() >= SINGULAR;
        let node_hat = if has_node {
            node.scale(1.0 / node.norm())
        } else {
            UnitVector::new(1.0, 0.0, 0.0)
        };
        let ahead = h_hat.cross(&node_hat).unit();
        OrbitPlane {
            h_hat,
            node_hat,
            ahead,
            has_node,
        }
    }
}

impl TemeState {
    /// Osculating Keplerian elements of this state.
    ///
    /// # Errors
    /// [`Error::NotBound`] if the state is on a parabolic or hyperbolic orbit.
    pub fn keplerian(&self) -> Result<KeplerianElements> {
        let mu = mu();
        let (r, v) = (self.position, self.velocity);
        let r_norm = r.norm();

        let energy = v.dot(&v) / 2.0 - mu / r_norm;
        if energy.partial_cmp(&0.0) != Some(std::cmp::Ordering::Less) {
            return Err(Error::NotBound);
        }
        let a = -mu / (2.0 * energy);

        let plane = self.orbit_plane();
        let (k_r, k_v) = (v.dot(&v) - mu / r_norm, r.dot(&v));
        let e_vec = Axis::new(
            (k_r * r.x - k_v * v.x) / mu,
            (k_r * r.y - k_v * v.y) / mu,
            (k_r * r.z - k_v * v.z) / mu,
        );
        let e = e_vec.norm();
        let raan = if plane.has_node {
            plane.node_hat.y.atan2(plane.node_hat.x)
        } else {
            0.0
        };

        let argp = if e < SINGULAR {
            0.0
        } else {
            plane.angle(&e_vec)
        };
        let nu = plane.angle(&r) - argp;
        let ecc_anomaly =
            2.0 * ((1.0 - e).sqrt() * (nu / 2.0).sin()).atan2((1.0 + e).sqrt() * (nu / 2.0).cos());
        let n = (mu / a.powi(3)).sqrt();

        Ok(KeplerianElements {
            semi_major_axis: a,
            eccentricity: e,
            inclination: Radians(plane.h_hat.z.clamp(-1.0, 1.0).acos()),
            raan: Radians(raan).normalized(),
            argument_of_perigee: Radians(argp).normalized(),
            true_anomaly: Radians(nu).normalized(),
            mean_anomaly: Radians(ecc_anomaly - e * ecc_anomaly.sin()).normalized(),
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
        Ok(Radians(plane.angle(&s.position)).normalized())
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
