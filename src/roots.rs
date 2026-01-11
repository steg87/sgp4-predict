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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("derivative dfx too small, unsafe")]
    Unstable,
    #[error("failed to converge after {0} iterations")]
    FailedToConverge(usize),
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
}
