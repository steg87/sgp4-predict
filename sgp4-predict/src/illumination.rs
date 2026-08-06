//! Satellite illumination (sunlit/eclipse) detection and iteration.
//!
//! Uses a cylindrical Earth shadow model: the satellite is in eclipse when
//! it is on the anti-Sun side of Earth and its perpendicular distance from
//! the Earth–Sun axis is less than one Earth radius. Shadow boundaries are
//! located with a fixed scan (60 seconds by default) and refined to
//! millisecond accuracy with the bracketed hybrid solver.
//!
//! [`IlluminationIter`] is a thin wrapper over the generic
//! [`WindowIter`](crate::WindowIter) in its partition mode
//! (`include_negative_windows` + `include_leading_partial` +
//! `clamp_to_interval`): the event function is
//! the shadow value (positive in eclipse) and every instant belongs to a
//! sunlit or eclipse window.
//!
//! [`Illumination`] implements [`IntervalRange`], so illumination windows
//! can be passed directly to prediction and observation iterators.
//!
//! [`IntervalRange`]: crate::IntervalRange

use chrono::{DateTime, Duration, Utc};

use crate::{
    Predictor, Result,
    detect::{EventFunction, FixedStep, MIN_POSITIVE_STEP, Sample, WindowIter},
    frames,
    frames::WGS84_A,
    roots::Refinement,
    time,
};

/// Tuning knobs for [`IlluminationIter`]'s coarse scan and window walk.
///
/// Pass a customised value to
/// [`Predictor::illumination_iter_with_opts`](crate::Predictor::illumination_iter_with_opts).
#[derive(Debug, Clone, Copy)]
pub struct IlluminationIterOpts {
    /// Fixed step used to scan for shadow-boundary crossings.
    pub step: Duration,
    /// Fixed step used to walk outward from a coarse sample to pin down a
    /// window's true start and end.
    pub walk_step: Duration,
    /// An eclipse window longer than this is reported as
    /// [`DetectError::WindowTooLong`](crate::DetectError::WindowTooLong).
    /// Sunlit windows are the gaps between resolved eclipse windows and are
    /// never bounded by this cap.
    pub max_window_duration: Duration,
}

impl Default for IlluminationIterOpts {
    fn default() -> Self {
        Self {
            step: Duration::seconds(60),
            walk_step: Duration::seconds(30),
            max_window_duration: Duration::hours(1),
        }
    }
}

/// Whether the satellite is in sunlight or in Earth's shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IlluminationState {
    /// The satellite is illuminated by the Sun.
    Sunlit,
    /// The satellite is in Earth's shadow (cylindrical umbra model).
    Eclipse,
}

/// A contiguous window of constant illumination state.
///
/// Implements [`IntervalRange`](crate::IntervalRange), so it can be passed
/// directly to prediction and observation iterators, and
/// [`TimeWindow`](crate::TimeWindow) for
/// [`clamp`](crate::TimeWindow::clamp), which preserves the `state`.
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

impl time::TimeWindow for Illumination {
    fn with_bounds(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            start,
            end,
            ..*self
        }
    }
}

/// Event function: the cylindrical-shadow value (positive in eclipse,
/// negative when sunlit).
pub(crate) struct ShadowFunction {
    predictor: Predictor,
}

impl EventFunction for ShadowFunction {
    fn sample(&mut self, t: DateTime<Utc>) -> Result<Sample> {
        Ok(Sample {
            time: t,
            value: shadow_value(&self.predictor, t)?,
            rate: None,
        })
    }
}

/// Iterator over sunlit and eclipse windows within a time interval.
///
/// Scans with a fixed step (60 seconds by default) and refines
/// shadow-boundary crossings to millisecond accuracy.
///
/// Windows that extend beyond the search interval are clamped to its boundaries:
/// the first window always starts at `interval.start` and the last always ends at
/// `interval.end`, regardless of when the illumination state actually changed.
pub struct IlluminationIter {
    inner: WindowIter<ShadowFunction, FixedStep>,
}

impl IlluminationIter {
    pub fn new(
        predictor: Predictor,
        interval: impl time::IntervalRange,
        opts: IlluminationIterOpts,
        refinement: Refinement,
    ) -> Self {
        let inner = WindowIter::builder()
            .interval(interval)
            .event_function(ShadowFunction { predictor })
            .step(FixedStep(opts.step.max(MIN_POSITIVE_STEP)))
            .walk_step(opts.walk_step)
            .max_window_duration(opts.max_window_duration)
            .include_negative_windows()
            .include_leading_partial()
            .clamp_to_interval()
            .refinement(refinement)
            .build()
            .expect("interval is always supplied");
        Self { inner }
    }
}

impl Predictor {
    /// Determine whether the satellite is sunlit or in eclipse at time t.
    ///
    /// Uses a cylindrical Earth shadow model: the satellite is in eclipse when it
    /// is on the anti-Sun side of Earth and within one Earth radius of the
    /// Earth–Sun axis.
    pub fn illumination_state(&self, t: DateTime<Utc>) -> Result<IlluminationState> {
        Ok(if shadow_value(self, t)? < 0.0 {
            IlluminationState::Sunlit
        } else {
            IlluminationState::Eclipse
        })
    }

    /// Detect all sunlit and eclipse windows over a time interval.
    ///
    /// Returns an iterator over illumination windows, each clamped to the search
    /// interval. Uses a cylindrical Earth shadow model with 60-second scan steps,
    /// refining shadow-boundary crossings to millisecond accuracy.
    pub fn illumination_iter(&self, interval: impl time::IntervalRange) -> IlluminationIter {
        self.illumination_iter_with_opts(interval, IlluminationIterOpts::default(), self.refinement)
    }

    /// Like [`Predictor::illumination_iter`], but with customized coarse-scan
    /// tuning and root-finder configuration. See [`IlluminationIterOpts`] and
    /// [`Refinement`].
    pub fn illumination_iter_with_opts(
        &self,
        interval: impl time::IntervalRange,
        opts: IlluminationIterOpts,
        refinement: Refinement,
    ) -> IlluminationIter {
        IlluminationIter::new(self.clone(), interval, opts, refinement)
    }
}

impl Iterator for IlluminationIter {
    type Item = Result<Illumination>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(match self.inner.next()? {
            Ok(window) => {
                let illumination = Illumination {
                    start: window.start,
                    end: window.end,
                    state: if window.positive {
                        IlluminationState::Eclipse
                    } else {
                        IlluminationState::Sunlit
                    },
                };
                tracing::debug!(
                    state = ?illumination.state,
                    start = %illumination.start,
                    end = %illumination.end,
                    "illumination window"
                );
                Ok(illumination)
            }
            Err(e) => {
                tracing::warn!("error calculating illumination window: {}", e.to_string());
                Err(e)
            }
        })
    }
}

/// Shadow value function for the cylindrical Earth shadow model.
///
/// Returns a negative value when the satellite is sunlit and a positive value
/// when it is in eclipse. Zero corresponds to the shadow boundary, so the
/// root finder can find exact crossing times.
fn shadow_value(predictor: &Predictor, t: DateTime<Utc>) -> Result<f64> {
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
