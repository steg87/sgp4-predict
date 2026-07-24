//! Type-safe angle newtypes.
//!
//! [`Degrees`] and [`Radians`] tag a plain `f64` with its unit so the two
//! can't be silently mixed up at a function boundary. There is deliberately
//! no `From<f64>` for either: that would let a bare number become "the right
//! unit" with no visible tag at the call site, which is exactly the mistake
//! these types exist to prevent. Construction is always explicit —
//! `Degrees(51.5)` or `Radians(1.2)` — and conversion between the two goes
//! through [`Degrees::to_radians`]/[`Radians::to_degrees`] (or the
//! corresponding `From` impls).
//!
//! The `.0` field is `pub` for ergonomic construction and pattern matching,
//! not as an enforced boundary — `Degrees(r.to_f64())` compiles just fine.
//! The safety this module buys is against *accidental* unit mixups (passing
//! a raw f64 where a specific unit was expected), not against a caller
//! deliberately reaching past the type.

use std::{cmp::Ordering, fmt};

/// An angle in degrees.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Degrees(pub f64);

/// An angle in radians.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Radians(pub f64);

impl Degrees {
    /// Convert to radians.
    pub fn to_radians(self) -> Radians {
        Radians(self.0.to_radians())
    }

    /// The underlying value.
    pub fn to_f64(self) -> f64 {
        self.0
    }

    /// Shorthand for `.to_radians().to_f64()`.
    pub fn radians(self) -> f64 {
        self.0.to_radians()
    }

    /// Normalize into `[0, 360)`.
    pub fn normalized(self) -> Self {
        Self(self.0.rem_euclid(360.0))
    }

    /// Total ordering, delegating to `f64::total_cmp`.
    pub fn total_cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Radians {
    /// Convert to degrees.
    pub fn to_degrees(self) -> Degrees {
        Degrees(self.0.to_degrees())
    }

    /// The underlying value.
    pub fn to_f64(self) -> f64 {
        self.0
    }

    /// Shorthand for `.to_degrees().to_f64()`.
    pub fn degrees(self) -> f64 {
        self.0.to_degrees()
    }

    /// Normalize into `[0, 2π)`.
    pub fn normalized(self) -> Self {
        Self(self.0.rem_euclid(std::f64::consts::TAU))
    }

    /// Total ordering, delegating to `f64::total_cmp`.
    pub fn total_cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl From<Degrees> for Radians {
    fn from(d: Degrees) -> Self {
        d.to_radians()
    }
}

impl From<Radians> for Degrees {
    fn from(r: Radians) -> Self {
        r.to_degrees()
    }
}

// `Display` deliberately prints the bare number with no unit suffix — CLI
// column widths and `{:.2}`-style precision specs forward straight through
// to the inner f64. `Debug` (derived above) is the one that shows the tag;
// don't "helpfully" add a °/rad suffix here, it'll break format-based output.
impl fmt::Display for Degrees {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Display for Radians {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_degrees_to_radians() {
        let r = Degrees(180.0).to_radians();
        assert!((r.0 - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn test_radians_to_degrees() {
        let d = Radians(std::f64::consts::PI).to_degrees();
        assert!((d.0 - 180.0).abs() < 1e-12);
    }

    #[test]
    fn test_roundtrip_via_from() {
        let d = Degrees(51.5074);
        let r: Radians = d.into();
        let back: Degrees = r.into();
        assert!((back.0 - d.0).abs() < 1e-12);
    }

    #[test]
    fn test_display_forwards_precision() {
        assert_eq!(format!("{:.2}", Degrees(5.1234)), "5.12");
        assert_eq!(format!("{:.2}", Radians(5.1234)), "5.12");
    }

    #[test]
    fn test_degrees_shorthand_matches_two_step() {
        let d = Degrees(90.0);
        assert_eq!(d.radians(), d.to_radians().to_f64());
    }

    #[test]
    fn test_radians_shorthand_matches_two_step() {
        let r = Radians(std::f64::consts::FRAC_PI_2);
        assert_eq!(r.degrees(), r.to_degrees().to_f64());
    }

    #[test]
    fn test_degrees_normalized_wraps_into_0_360() {
        assert!((Degrees(370.0).normalized().0 - 10.0).abs() < 1e-12);
        assert!((Degrees(-10.0).normalized().0 - 350.0).abs() < 1e-12);
    }

    #[test]
    fn test_radians_normalized_wraps_into_0_tau() {
        let tau = std::f64::consts::TAU;
        assert!((Radians(tau + 1.0).normalized().0 - 1.0).abs() < 1e-12);
        assert!(Radians(-1.0).normalized().0 > 0.0);
    }

    #[test]
    fn test_total_cmp_orders_by_value() {
        assert_eq!(Degrees(1.0).total_cmp(&Degrees(2.0)), Ordering::Less);
        assert_eq!(Radians(2.0).total_cmp(&Radians(1.0)), Ordering::Greater);
    }
}
