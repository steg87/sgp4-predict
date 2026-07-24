//! Root-finding algorithms used to refine satellite event times.
//!
//! [`Refinement`] is the solver used throughout the crate: a bracketed
//! hybrid that takes Newton-Raphson steps whenever a sample supplies a
//! derivative and falls back to secant/bisection steps otherwise, deciding
//! per iteration. It is passed to
//! [`Predictor::with_refinement`](crate::Predictor::with_refinement) to
//! customise solver behaviour across the whole predictor.
use thiserror::Error as ThisError;

/// Bracketed hybrid root finder used to refine event times.
///
/// Each iteration chooses its step from the most recent sample:
/// a Newton-Raphson step when the sample supplies a derivative and the step
/// stays inside the bracket, a secant step through the bracket endpoints
/// otherwise, and bisection whenever an interpolated step leaves the bracket
/// or the same bracket side has been updated twice in a row (so one-sided
/// convergence cannot stall). The bracket never widens, so convergence is
/// guaranteed.
///
/// The convergence criterion is on the bracket width in **seconds**, making
/// it independent of the event function's units (radians, m/s, metres, …).
///
/// The algorithm is the safeguarded Newton–bisection hybrid `rtsafe` of
/// *Numerical Recipes* (Press et al., 3rd ed., §9.4), extended with a
/// secant step for derivative-free samples (Dekker's method) and with the
/// consecutive-same-side bisection rule, which serves the same
/// anti-stagnation purpose as the Illinois modification of regula falsi
/// (Dowell & Jarratt, 1971).
///
/// Pass to [`Predictor::with_refinement`](crate::Predictor::with_refinement)
/// to customise refinement across all detection iterators,
/// [`detect_transit`], and [`max_elevation`].
///
/// [`detect_transit`]: crate::Predictor::detect_transit
/// [`max_elevation`]: crate::Predictor::max_elevation
#[derive(Debug, Clone, Copy)]
pub struct Refinement {
    /// Convergence threshold on the bracket width, in seconds: iteration
    /// stops once the crossing is pinned down to within this duration.
    pub time_tolerance: f64,
    /// Maximum number of iterations before returning `Error::FailedToConverge`.
    pub max_iter: usize,
}

impl Default for Refinement {
    fn default() -> Self {
        Self {
            time_tolerance: 1e-3,
            max_iter: 100,
        }
    }
}

impl Refinement {
    /// Find a root of `f` bracketed in `[a, b]` (times as f64 seconds).
    ///
    /// `f(x)` returns `(value, rate)`, where `rate` is the time derivative
    /// of the value when cheaply available; `f(a)` and `f(b)` must have
    /// opposite signs. Returns a point within
    /// [`time_tolerance`](Refinement::time_tolerance) of a sign change.
    pub fn solve<F, E>(&self, a: f64, b: f64, mut f: F) -> std::result::Result<f64, Error>
    where
        F: FnMut(f64) -> std::result::Result<(f64, Option<f64>), E>,
        E: std::error::Error,
    {
        let (fa, _) = f(a).map_err(|e| Error::CostFn(e.to_string()))?;
        if fa == 0.0 {
            return Ok(a);
        }
        let (fb, rb) = f(b).map_err(|e| Error::CostFn(e.to_string()))?;
        if fb == 0.0 {
            return Ok(b);
        }
        if (fa > 0.0) == (fb > 0.0) {
            return Err(Error::Unbracketed);
        }

        // Normalise the bracket to lo < hi. The current iterate (x, fx, rx)
        // is always the most recently evaluated point.
        let (mut lo, mut flo, mut hi, mut fhi) = if a <= b {
            (a, fa, b, fb)
        } else {
            (b, fb, a, fa)
        };
        let (mut x, mut fx, mut rx) = (b, fb, rb);

        // Consecutive updates to the same bracket side; two in a row forces
        // bisection so the untouched side cannot pin the bracket open.
        let mut same_side = 0u32;
        let mut last_updated_lo: Option<bool> = None;

        for _ in 0..self.max_iter {
            if hi - lo < self.time_tolerance {
                // Converged: return the bracket endpoint closest to the root.
                return Ok(if flo.abs() <= fhi.abs() { lo } else { hi });
            }

            let mut candidate = None;
            if same_side < 2 {
                if let Some(r) = rx {
                    // Newton step from the latest sample; a zero rate yields
                    // a non-finite step and fails the range check below.
                    let newton = x - fx / r;
                    if newton > lo && newton < hi {
                        candidate = Some(newton);
                    }
                }
                if candidate.is_none() {
                    // Secant step through the bracket endpoints (fhi and flo
                    // have opposite signs, so the denominator is non-zero).
                    let secant = (lo * fhi - hi * flo) / (fhi - flo);
                    if secant > lo && secant < hi {
                        candidate = Some(secant);
                    }
                }
            }
            // Keep the step at least half a tolerance from either endpoint
            // so the bracket shrinks meaningfully every iteration.
            let margin = 0.5 * self.time_tolerance;
            let x_new = candidate
                .unwrap_or(0.5 * (lo + hi))
                .clamp(lo + margin, hi - margin);

            let (fx_new, r_new) = f(x_new).map_err(|e| Error::CostFn(e.to_string()))?;
            if fx_new == 0.0 {
                return Ok(x_new);
            }

            let updates_lo = (fx_new > 0.0) == (flo > 0.0);
            if updates_lo {
                (lo, flo) = (x_new, fx_new);
            } else {
                (hi, fhi) = (x_new, fx_new);
            }
            same_side = if last_updated_lo == Some(updates_lo) {
                same_side + 1
            } else {
                0
            };
            last_updated_lo = Some(updates_lo);
            (x, fx, rx) = (x_new, fx_new, r_new);
        }
        Err(Error::FailedToConverge {
            iterations: self.max_iter,
            tolerance: self.time_tolerance,
            result: 0.5 * (lo + hi),
            error: fx.abs(),
        })
    }
}

/// Errors returned by the root-finding algorithms.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("failed to converge after {iterations} iterations")]
    FailedToConverge {
        iterations: usize,
        tolerance: f64,
        result: f64,
        error: f64,
    },
    #[error("root is not bracketed")]
    Unbracketed,
    #[error("cost function error: {0}")]
    CostFn(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    // Minimal error type for cost-function failure tests.
    #[derive(Debug)]
    struct TestError(&'static str);
    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for TestError {}

    // --- Refinement::solve ---

    #[test]
    fn test_refinement_solve_with_rate() {
        // f(x) = x² − 4 with derivative: root at x = 2, bracketed in [1, 3.5].
        let r = Refinement::default();
        let root = r
            .solve(1.0, 3.5, |x| {
                Ok::<_, Infallible>((x * x - 4.0, Some(2.0 * x)))
            })
            .unwrap();
        assert!((root - 2.0).abs() < r.time_tolerance);
    }

    #[test]
    fn test_refinement_solve_without_rate() {
        // f(x) = x³ − 8 with no derivative: secant/bisection only.
        let r = Refinement::default();
        let root = r
            .solve(0.0, 3.0, |x| Ok::<_, Infallible>((x.powi(3) - 8.0, None)))
            .unwrap();
        assert!((root - 2.0).abs() < r.time_tolerance);
    }

    #[test]
    fn test_refinement_solve_intermittent_rate() {
        // The rate is only available on every other evaluation; each
        // iteration must adapt to what the sample actually carries.
        let mut n = 0u32;
        let r = Refinement::default();
        let root = r
            .solve(0.0, 3.0, |x| {
                n += 1;
                let rate = n.is_multiple_of(2).then_some(3.0 * x * x);
                Ok::<_, Infallible>((x.powi(3) - 8.0, rate))
            })
            .unwrap();
        assert!((root - 2.0).abs() < r.time_tolerance);
    }

    #[test]
    fn test_refinement_solve_stall_resistant() {
        // f(x) = x¹⁰ − ½ on [0, 1]: plain regula falsi stalls badly here;
        // the forced-bisection rule must keep the bracket shrinking.
        let r = Refinement {
            time_tolerance: 1e-9,
            max_iter: 100,
        };
        let root = r
            .solve(0.0, 1.0, |x| Ok::<_, Infallible>((x.powi(10) - 0.5, None)))
            .unwrap();
        assert!((root - 0.5_f64.powf(0.1)).abs() < 1e-8);
    }

    #[test]
    fn test_refinement_solve_bad_rate_still_converges() {
        // A wrong-signed rate sends Newton steps out of the bracket; the
        // bracketed fallbacks must still find the root of f(x) = x − 0.5.
        let r = Refinement::default();
        let root = r
            .solve(0.0, 2.0, |x| Ok::<_, Infallible>((x - 0.5, Some(-1.0))))
            .unwrap();
        assert!((root - 0.5).abs() < r.time_tolerance);
    }

    #[test]
    fn test_refinement_solve_exact_zero_endpoint() {
        let r = Refinement::default();
        let root = r
            .solve(2.0, 3.0, |x| Ok::<_, Infallible>((x - 2.0, None)))
            .unwrap();
        assert_eq!(root, 2.0);
    }

    #[test]
    fn test_refinement_solve_unbracketed() {
        let result =
            Refinement::default().solve(2.4, 3.0, |x| Ok::<_, Infallible>((x.powi(3) - 8.0, None)));
        assert!(matches!(result, Err(Error::Unbracketed)));
    }

    #[test]
    fn test_refinement_solve_max_iterations() {
        // Three iterations cannot narrow [0, 3] to 1e-12.
        let r = Refinement {
            time_tolerance: 1e-12,
            max_iter: 3,
        };
        let result = r.solve(0.0, 3.0, |x| Ok::<_, Infallible>((x.powi(3) - 8.0, None)));
        assert!(matches!(
            result,
            Err(Error::FailedToConverge { iterations: 3, .. })
        ));
    }

    #[test]
    fn test_refinement_solve_cost_fn_error_propagates() {
        let result = Refinement::default().solve(0.0, 1.0, |_| {
            Err::<(f64, Option<f64>), _>(TestError("boom"))
        });
        assert!(matches!(result, Err(Error::CostFn(_))));
    }
}
