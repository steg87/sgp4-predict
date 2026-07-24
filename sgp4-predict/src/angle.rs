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

use std::fmt;

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
}
