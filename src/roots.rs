use thiserror::Error as ThisError;

/// Newton–Raphson root finder
///
/// - `x0`: initial guess
/// - `f`: f(x) -> (y, dy)
/// - `tol`: acceptable absolute error in f(x)
/// - `max_iter`: safety cap
///
/// Returns `Some(root)` on success, or `None` if it fails to converge.
pub fn newton_raphson<F>(mut x0: f64, mut f: F, tol: f64, max_iter: usize) -> Result<f64, Error>
where
    F: FnMut(f64) -> (f64, f64),
{
    for _ in 0..max_iter {
        let (fx, dfx) = f(x0);
        if fx.abs() < tol {
            return Ok(x0);
        }

        if dfx.abs() < 1e-12 {
            // derivative too small, can't continue safely
            return Err(Error::Unstable);
        }

        // Newton step
        x0 -= fx / dfx;
    }
    Err(Error::FailedToConverge(max_iter))
}

/// Brent root finder. Optimally combines bisection (guaranteed convergence), Secant method and
/// inverse quadratic interpolation.
///
/// - `a`: bracket start
/// - `b`: bracket end
/// - `f`: f(x) -> y
/// - `tol`: acceptable absolute error in f(x)
/// - `max_iter`: safety cap
pub fn brent<F>(mut a: f64, mut b: f64, f: F, tol: f64, max_iter: usize) -> Result<f64, Error>
where
    F: Fn(f64) -> f64,
{
    let mut fa = f(a);
    let mut fb = f(b);

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

    for _ in 0..max_iter {
        if fb.abs() < tol {
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

        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
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

        fb = f(b);
    }
    Err(Error::FailedToConverge(max_iter))
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("derivative dfx too small, unsafe")]
    Unstable,
    #[error("failed to converge after {0} iterations")]
    FailedToConverge(usize),
    #[error("root is not bracketed")]
    Unbracketed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_newton_raphson_cubic() {
        // f(x) = x³ - 8, f'(x) = 3x²
        // Root at x = 2
        let f = |x: f64| (x.powi(3) - 8.0, 3.0 * x.powi(2));

        let result = newton_raphson(1.0, f, 1e-6, 10);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_newton_raphson_converges() {
        // f(x) = x² - 4, f'(x) = 2x
        // Roots at x = ±2
        let f = |x: f64| (x * x - 4.0, 2.0 * x);

        let result = newton_raphson(1.0, f, 1e-9, 10);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_newton_raphson_unstable() {
        // Force zero f'(x), triggering instability
        let f = |_x: f64| (1.0, 0.0);

        let result = newton_raphson(1.0, f, 1e-6, 10);
        assert!(matches!(result, Err(Error::Unstable)));
    }

    #[test]
    fn test_newton_raphson_max_iterations() {
        // Pathological case that won't converge
        let f = |_x: f64| (1.0, 1.0);

        let result = newton_raphson(0.0, f, 1e-6, 10);
        assert!(matches!(result, Err(Error::FailedToConverge(10))));
    }

    #[test]
    fn test_brent_cubic() {
        // f(x) = x³ - 8, f'(x) = 3x²
        // Root at x = 2
        let f = |x: f64| x.powi(3) - 8.0;

        let result = brent(0.0, 3.0, f, 1e-6, 20);
        assert!(result.is_ok());
        assert!((result.unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_brent_converges() {
        // f(x) = x² - 4, f'(x) = 2x
        // Roots at x = ±2
        let f = |x: f64| x * x - 4.0;

        assert!(brent(-3.1, -0.6, f, 1e-6, 20).unwrap() + 2.0 < 1e-6);
        assert!(brent(1.5, 3.9, f, 1e-6, 20).unwrap() - 2.0 < 1e-6);
    }

    #[test]
    fn test_brent_bracketing_error() {
        let f = |x: f64| x.powi(3) - 8.0;

        let result = brent(2.4, 3.0, f, 1e-6, 10);
        assert!(matches!(result, Err(Error::Unbracketed)));
    }

    #[test]
    fn test_brent_max_iterations() {
        let f = |x: f64| x.powi(3) - 8.0;

        let result = brent(0.0, 3.0, f, 1e-6, 10);
        assert!(matches!(result, Err(Error::FailedToConverge(10))));
    }
}
