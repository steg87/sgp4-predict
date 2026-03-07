//! Satellite illumination (sunlit/eclipse) detection and iteration.
//!
//! Uses a cylindrical Earth shadow model: the satellite is in eclipse when
//! it is on the anti-Sun side of Earth and its perpendicular distance from
//! the Earth–Sun axis is less than one Earth radius. Shadow boundaries are
//! located with a 60-second scan and refined to sub-millisecond accuracy
//! with Brent's method.
//!
//! [`Illumination`] implements [`IntervalRange`], so illumination windows
//! can be passed directly to prediction and observation iterators.
//!
//! [`IntervalRange`]: crate::IntervalRange

use chrono::{DateTime, Duration, Utc};
use std::ops::Range;

use crate::{Error, Predictor, Result, frames, roots, time};
use roots::Brent;

const STEP: Duration = Duration::seconds(60);

/// Earth's equatorial radius (WGS-84), metres.
/// Used as the radius of the cylindrical shadow.
const WGS84_A: f64 = 6_378_137.0;

/// Whether the satellite is in sunlight or in Earth's shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IlluminationState {
    /// The satellite is illuminated by the Sun.
    Sunlit,
    /// The satellite is in Earth's shadow (cylindrical umbra model).
    Eclipse,
}

impl IlluminationState {
    fn opposite(self) -> Self {
        match self {
            Self::Sunlit => Self::Eclipse,
            Self::Eclipse => Self::Sunlit,
        }
    }
}

/// A contiguous window of constant illumination state.
#[derive(Debug, Clone, Copy)]
pub struct Illumination {
    /// Start of the window (inclusive).
    pub start: DateTime<Utc>,
    /// End of the window (exclusive).
    pub end: DateTime<Utc>,
    /// Illumination state throughout this window.
    pub state: IlluminationState,
}

impl time::IntervalRange for Illumination {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

/// Iterator over sunlit and eclipse windows within a time interval.
///
/// Scans with a fixed 60-second step and refines shadow-boundary crossings with
/// Brent's method to sub-millisecond accuracy.
///
/// Windows that extend beyond the search interval are clamped to its boundaries:
/// the first window always starts at `interval.start` and the last always ends at
/// `interval.end`, regardless of when the illumination state actually changed.
pub struct IlluminationIter {
    predictor: Predictor,
    interval: Range<DateTime<Utc>>,
    next_time: DateTime<Utc>,
    window_start: DateTime<Utc>,
    current: Option<IlluminationState>,
    prev: Option<(f64, f64)>, // (t as f64, shadow_value) at previous scan point
    finished: bool,
    brent: Brent,
}

impl IlluminationIter {
    pub fn new(predictor: Predictor, interval: impl time::IntervalRange) -> Self {
        Self {
            predictor,
            interval: interval.start()..interval.end(),
            next_time: interval.start(),
            window_start: interval.start(),
            current: None,
            prev: None,
            finished: false,
            brent: Brent::default(),
        }
    }

    /// Override the Brent solver configuration used to refine shadow-boundary crossings.
    pub fn with_brent(mut self, b: Brent) -> Self {
        self.brent = b;
        self
    }
}

impl Iterator for IlluminationIter {
    type Item = Result<Illumination>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        while self.interval.contains(&self.next_time) {
            let t = self.next_time;
            let t_f64 = time::datetime_to_f64(t);

            let sv = match shadow_value(&self.predictor, t) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            if let Some((prev_t, prev_sv)) = self.prev {
                if (prev_sv > 0.0) != (sv > 0.0) {
                    // Sign change detected — find the shadow-boundary crossing.
                    // If either scan point is exactly on the boundary (value == 0.0),
                    // Brent's bracket would be degenerate; the scan point itself is
                    // the crossing and no root-finding is needed.
                    let crossing = if sv == 0.0 {
                        t
                    } else if prev_sv == 0.0 {
                        time::f64_to_datetime(prev_t)
                    } else {
                        // Refine crossing with Brent's method.
                        let predictor = self.predictor.clone();
                        let crossing_f64 = match self.brent.solve(prev_t, t_f64, |x| {
                            shadow_value(&predictor, time::f64_to_datetime(x))
                        }) {
                            Ok(t) => t,
                            Err(e) => return Some(Err(Error::Roots(e))),
                        };
                        time::f64_to_datetime(crossing_f64)
                    };
                    let state = self.current.expect("current initialized with prev");
                    let window = Illumination {
                        start: self.window_start,
                        end: crossing,
                        state,
                    };
                    self.window_start = crossing;
                    // Derive the new state directly from sv rather than
                    // using state.opposite(), so the state is always
                    // grounded in the actual shadow-function value and
                    // cannot accumulate error across multiple crossings.
                    // When sv is exactly 0.0 the scan point is itself the
                    // crossing, so sv provides no directional information
                    // and opposite() is the only option.
                    self.current = Some(if sv > 0.0 {
                        IlluminationState::Eclipse
                    } else if sv < 0.0 {
                        IlluminationState::Sunlit
                    } else {
                        state.opposite()
                    });
                    self.prev = Some((t_f64, sv));
                    self.next_time += STEP;
                    return Some(Ok(window));
                }
            } else {
                // First sample: determine initial illumination state.
                self.current = Some(if sv > 0.0 {
                    IlluminationState::Eclipse
                } else {
                    IlluminationState::Sunlit
                });
                self.window_start = self.interval.start;
            }

            self.prev = Some((t_f64, sv));
            self.next_time += STEP;
        }

        // End of interval — yield the final (possibly partial) window.
        if let Some(state) = self.current {
            self.finished = true;
            return Some(Ok(Illumination {
                start: self.window_start,
                end: self.interval.end,
                state,
            }));
        }

        // Interval was empty — nothing to yield.
        None
    }
}

/// Shadow value function for the cylindrical Earth shadow model.
///
/// Returns a negative value when the satellite is sunlit and a positive value
/// when it is in eclipse. Zero corresponds to the shadow boundary, so Brent's
/// method can find exact crossing times.
pub(crate) fn shadow_value(predictor: &Predictor, t: DateTime<Utc>) -> Result<f64> {
    let state = predictor.propagate(t)?;
    Ok(shadow_fn(
        state.position.x,
        state.position.y,
        state.position.z,
        t,
    ))
}

/// Evaluate the cylindrical-shadow scalar for a satellite position in TEME.
///
/// The cylindrical model treats Earth's shadow as an infinite cylinder of radius
/// `R_Earth` aligned with the Earth–Sun axis. A satellite is in eclipse when:
///   1. It is on the anti-Sun side of Earth (`d_sun < 0`), and
///   2. Its perpendicular distance from the shadow axis is less than `R_Earth`.
///
/// Returns:
///   - Negative: satellite is sunlit.
///   - Positive: satellite is in eclipse (shadow).
///   - Zero: shadow boundary.
fn shadow_fn(px: f64, py: f64, pz: f64, t: DateTime<Utc>) -> f64 {
    let sun = frames::sun_position_eci(t);
    let sun_mag = (sun[0].powi(2) + sun[1].powi(2) + sun[2].powi(2)).sqrt();
    let sun_hat = [sun[0] / sun_mag, sun[1] / sun_mag, sun[2] / sun_mag];

    let r_sq = px.powi(2) + py.powi(2) + pz.powi(2);
    // Projection of satellite position onto the Sun direction.
    let d_sun = px * sun_hat[0] + py * sun_hat[1] + pz * sun_hat[2];

    if d_sun >= 0.0 {
        // Satellite is on the same side as the Sun — always sunlit.
        // Return a strongly negative value (magnitude ≈ orbital altitude).
        return WGS84_A - r_sq.sqrt();
    }

    // Satellite is on the anti-Sun side: check perpendicular distance from shadow axis.
    let d_perp = (r_sq - d_sun * d_sun).max(0.0).sqrt();
    WGS84_A - d_perp // positive ⟹ eclipse, negative ⟹ sunlit
}

#[cfg(test)]
mod tests {
    use super::{WGS84_A, shadow_fn};
    use crate::frames::sun_position_eci;
    use chrono::{TimeZone, Utc};

    /// Normalise a 3-vector and return the unit vector.
    fn normalise(v: [f64; 3]) -> [f64; 3] {
        let mag = (v[0].powi(2) + v[1].powi(2) + v[2].powi(2)).sqrt();
        [v[0] / mag, v[1] / mag, v[2] / mag]
    }

    /// A vector perpendicular to `v`, found via cross product with (0, 0, 1)
    /// or (1, 0, 0) if v is nearly parallel to the z-axis.
    fn perpendicular(v: [f64; 3]) -> [f64; 3] {
        let cross = if v[2].abs() < 0.9 {
            [
                v[1] * 0.0 - v[2] * 0.0,
                v[2] * 1.0 - v[0] * 0.0,
                v[0] * 0.0 - v[1] * 1.0,
            ]
            // cross(v, z) = [vy*0 - vz*0, vz*1 - vx*0, vx*0 - vy*1] — simplified:
        } else {
            // cross(v, x) when v is near z-axis
            [0.0, v[2] * 1.0 - v[1] * 0.0, v[1] * 0.0 - v[2] * 0.0]
        };
        // cross(v, z_hat) = (vy, -vx, 0)
        let perp = if v[2].abs() < 0.9 {
            [-v[1], v[0], 0.0]
        } else {
            [0.0, -v[2], v[1]]
        };
        let _ = cross; // suppress unused warning from the first branch above
        normalise(perp)
    }

    #[test]
    fn test_shadow_fn_sunlit_sun_side() {
        // Satellite placed in the direction of the Sun at LEO altitude (d_sun > 0)
        // must always be sunlit (shadow_fn < 0).
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
        let sun_hat = normalise(sun_position_eci(t));
        let r = 7_000_000.0_f64; // 7 000 km
        let sv = shadow_fn(sun_hat[0] * r, sun_hat[1] * r, sun_hat[2] * r, t);
        assert!(
            sv < 0.0,
            "satellite on sun-side should be sunlit (got {sv})"
        );
    }

    #[test]
    fn test_shadow_fn_eclipse_on_axis() {
        // Satellite placed directly behind Earth on the shadow axis (d_sun < 0,
        // d_perp = 0) must be in eclipse (shadow_fn > 0).
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
        let sun_hat = normalise(sun_position_eci(t));
        let r = 7_000_000.0_f64;
        let sv = shadow_fn(-sun_hat[0] * r, -sun_hat[1] * r, -sun_hat[2] * r, t);
        assert!(
            sv > 0.0,
            "satellite on shadow axis should be in eclipse (got {sv})"
        );
    }

    #[test]
    fn test_shadow_fn_sunlit_outside_cylinder() {
        // Satellite on the anti-Sun side but displaced 2 Earth-radii laterally
        // from the shadow axis must be sunlit (d_perp > WGS84_A → shadow_fn < 0).
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
        let sun_hat = normalise(sun_position_eci(t));
        let perp = perpendicular(sun_hat);

        let d_back = 1_000_000.0_f64; // 1 000 km behind Earth centre
        let d_lateral = 2.0 * WGS84_A; // twice Earth's radius — well outside the cylinder
        let px = -sun_hat[0] * d_back + perp[0] * d_lateral;
        let py = -sun_hat[1] * d_back + perp[1] * d_lateral;
        let pz = -sun_hat[2] * d_back + perp[2] * d_lateral;

        let sv = shadow_fn(px, py, pz, t);
        assert!(
            sv < 0.0,
            "satellite outside shadow cylinder should be sunlit (got {sv})"
        );
    }
}
