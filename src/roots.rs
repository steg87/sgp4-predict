use thiserror::Error as ThisError;

/// Newton–Raphson root finder.
///
/// Configured by `tolerance` (acceptable absolute error in f(x)) and `max_iter` (safety cap).
#[derive(Debug, Clone, Copy)]
pub struct NewtonRaphson {
    pub tolerance: f64,
    pub max_iter: usize,
}

impl Default for NewtonRaphson {
    fn default() -> Self {
        Self {
            tolerance: 1e-6,
            max_iter: 50,
        }
    }
}

impl NewtonRaphson {
    /// Find the root of `f` starting from `x0`.
    ///
    /// `f(x)` must return `(y, dy)` — the function value and its derivative — or an error.
    pub fn solve<F, E>(&self, x0: f64, mut f: F) -> Result<f64, Error>
    where
        F: FnMut(f64) -> Result<(f64, f64), E>,
        E: std::error::Error,
    {
        let mut x0 = x0;
        for _ in 0..self.max_iter {
            let (fx, dfx) = f(x0).map_err(|e| Error::CostFn(e.to_string()))?;
            if fx.abs() < self.tolerance {
                return Ok(x0);
            }

            if dfx.abs() < 1e-12 {
                // derivative too small, can't continue safely
                return Err(Error::Unstable);
            }

            // Newton step
            x0 -= fx / dfx;
        }
        let (fx, _) = f(x0).map_err(|e| Error::CostFn(e.to_string()))?;
        Err(Error::FailedToConverge {
            iterations: self.max_iter,
            tolerance: self.tolerance,
            result: x0,
            error: fx.abs(),
        })
    }
}

/// Brent root finder. Optimally combines bisection (guaranteed convergence), Secant method and
/// inverse quadratic interpolation.
///
/// Configured by `tolerance` (acceptable absolute error in f(x)) and `max_iter` (safety cap).
#[derive(Debug, Clone, Copy)]
pub struct Brent {
    pub tolerance: f64,
    pub max_iter: usize,
}

impl Default for Brent {
    fn default() -> Self {
        Self {
            tolerance: 1e-6,
            max_iter: 100,
        }
    }
}

impl Brent {
    /// Find the root of `f` bracketed in `[a, b]`.
    ///
    /// `f(a)` and `f(b)` must have opposite signs (i.e. the root is bracketed).
    pub fn solve<F, E>(&self, mut a: f64, mut b: f64, mut f: F) -> Result<f64, Error>
    where
        F: FnMut(f64) -> Result<f64, E>,
        E: std::error::Error,
    {
        let mut fa = f(a).map_err(|e| Error::CostFn(e.to_string()))?;
        let mut fb = f(b).map_err(|e| Error::CostFn(e.to_string()))?;

        if fa * fb >= 0.0 {
            return Err(Error::Unbracketed);
        }

        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }

        let mut c = a;
        let mut fc = fa;
        let mut d = b - a;
        let mut e = d;

        for _ in 0..self.max_iter {
            if fb.abs() < self.tolerance {
                return Ok(b);
            }

            if fa * fb > 0.0 {
                // Ensure [b, c] brackets the root
                a = c;
                fa = fc;
                d = b - a;
                e = d;
            }

            if fa.abs() < fb.abs() {
                c = b;
                fc = fb;
                b = a;
                fb = fa;
                a = c;
                fa = fc;
            }

            let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * self.tolerance;
            let m = 0.5 * (a - b);

            if m.abs() <= tol1 {
                return Ok(b);
            }

            if e.abs() >= tol1 && fc.abs() > fb.abs() {
                // Attempt interpolation
                let s = fb / fc;
                let (p, q) = if a == c {
                    // Secant method
                    (2.0 * m * s, 1.0 - s)
                } else {
                    // Inverse quadratic interpolation
                    let q1 = fc / fa;
                    let r = fb / fa;
                    (
                        s * (2.0 * m * q1 * (q1 - r) - (b - c) * (r - 1.0)),
                        (q1 - 1.0) * (r - 1.0) * (s - 1.0),
                    )
                };

                let mut p = p;
                let mut q = q;
                if p > 0.0 {
                    q = -q;
                }
                p = p.abs();

                let min1 = 3.0 * m * q.abs() - (tol1 * q).abs();
                let min2 = (e * q).abs();

                if 2.0 * p < min1.min(min2) {
                    // Accept interpolation
                    e = d;
                    d = p / q;
                } else {
                    // Fall back to bisection
                    d = m;
                    e = m;
                }
            } else {
                // Bisection
                d = m;
                e = m;
            }

            c = a;
            fc = fa;
            a = b;
            fa = fb;

            b += if d.abs() > tol1 { d } else { tol1.copysign(m) };

            fb = f(b).map_err(|e| Error::CostFn(e.to_string()))?;
        }
        Err(Error::FailedToConverge {
            iterations: self.max_iter,
            tolerance: self.tolerance,
            result: b,
            error: fb.abs(),
        })
    }
}

/// Combined solver for the Newton-Raphson → Brent fallback used in transit crossing refinement.
#[derive(Debug, Clone, Copy, Default)]
pub struct Refinement {
    pub newton_raphson: NewtonRaphson,
    pub brent: Brent,
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("derivative dfx too small, unsafe")]
    Unstable,
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

    #[test]
    fn test_newton_raphson_cubic() {
        // f(x) = x³ - 8, f'(x) = 3x²
        // Root at x = 2
        let f = |x: f64| Ok::<_, Infallible>((x.powi(3) - 8.0, 3.0 * x.powi(2)));

        let result = NewtonRaphson {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(1.0, f);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_newton_raphson_converges() {
        // f(x) = x² - 4, f'(x) = 2x
        // Roots at x = ±2
        let f = |x: f64| Ok::<_, Infallible>((x * x - 4.0, 2.0 * x));

        let result = NewtonRaphson {
            tolerance: 1e-9,
            max_iter: 10,
        }
        .solve(1.0, f);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_newton_raphson_unstable() {
        // Force zero f'(x), triggering instability
        let f = |_x: f64| Ok::<_, Infallible>((1.0, 0.0));

        let result = NewtonRaphson {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(1.0, f);
        assert!(matches!(result, Err(Error::Unstable)));
    }

    #[test]
    fn test_newton_raphson_max_iterations() {
        // Pathological case that won't converge
        let f = |_x: f64| Ok::<_, Infallible>((1.0, 1.0));

        let result = NewtonRaphson {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(0.0, f);
        assert!(matches!(
            result,
            Err(Error::FailedToConverge { iterations: 10, .. })
        ));
    }

    #[test]
    fn test_newton_raphson_cost_fn_error() {
        let f = |_x: f64| Err::<(f64, f64), _>(TestError("something went wrong"));

        let result = NewtonRaphson {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(0.0, f);
        assert!(matches!(result, Err(Error::CostFn(_))));
    }

    #[test]
    fn test_brent_cubic() {
        // f(x) = x³ - 8
        // Root at x = 2
        let f = |x: f64| Ok::<_, Infallible>(x.powi(3) - 8.0);

        let result = Brent {
            tolerance: 1e-6,
            max_iter: 20,
        }
        .solve(0.0, 3.0, f);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_brent_converges() {
        // f(x) = x² - 4
        // Roots at x = ±2
        assert!(
            Brent {
                tolerance: 1e-6,
                max_iter: 20
            }
            .solve(-3.1, -0.6, |x: f64| Ok::<_, Infallible>(x * x - 4.0))
            .unwrap()
                + 2.0
                < 1e-6
        );
        assert!(
            Brent {
                tolerance: 1e-6,
                max_iter: 20
            }
            .solve(1.5, 3.9, |x: f64| Ok::<_, Infallible>(x * x - 4.0))
            .unwrap()
                - 2.0
                < 1e-6
        );
    }

    #[test]
    fn test_brent_bracketing_error() {
        let f = |x: f64| Ok::<_, Infallible>(x.powi(3) - 8.0);

        let result = Brent {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(2.4, 3.0, f);
        assert!(matches!(result, Err(Error::Unbracketed)));
    }

    #[test]
    fn test_brent_max_iterations() {
        let f = |x: f64| Ok::<_, Infallible>(x.powi(3) - 8.0);

        let result = Brent {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(0.0, 3.0, f);
        assert!(matches!(
            result,
            Err(Error::FailedToConverge { iterations: 10, .. })
        ));
    }

    #[test]
    fn test_brent_cost_fn_error() {
        let f = |_x: f64| Err::<f64, _>(TestError("something went wrong"));

        let result = Brent {
            tolerance: 1e-6,
            max_iter: 10,
        }
        .solve(0.0, 3.0, f);
        assert!(matches!(result, Err(Error::CostFn(_))));
    }
}
