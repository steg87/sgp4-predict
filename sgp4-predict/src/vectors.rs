//! Generic vector types with compile-time coordinate-frame tracking.
//!
//! [`Position`], [`Velocity`] and [`UnitVector`] are type aliases over
//! [`Vec3`], distinguished by kind markers so they cannot be accidentally
//! mixed. [`StateVector`] pairs a position and a velocity and carries a
//! phantom frame type `F` — one of the TEME, ECEF, ENU or LVLH marker
//! structs — so the compiler rejects passing a vector in the wrong frame.
//!
//! All values are in SI units: metres for position, metres per second for
//! velocity.

use std::marker::PhantomData;

/// Backing 3-component vector type, parameterised by kind `K` and frame `F`.
///
/// Not used directly; prefer the [`Position`], [`Velocity`] and [`UnitVector`]
/// type aliases.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3<K, F> {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
    _marker: PhantomData<(K, F)>,
}

impl<K, F> Vec3<K, F> {
    /// Build a vector from its components.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            _marker: PhantomData,
        }
    }
}

impl<K, F> Vec3<K, F> {
    /// Dot product with a vector of any kind in the same frame.
    pub(crate) fn dot<K2>(&self, other: &Vec3<K2, F>) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub(crate) fn norm(&self) -> f64 {
        self.dot(self).sqrt()
    }

    pub(crate) fn scale(&self, k: f64) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }

    /// Cross product. The result is neither of its operands' kinds, so it is
    /// an [`Axis`] until normalised.
    pub(crate) fn cross<K2>(&self, o: &Vec3<K2, F>) -> Axis<F> {
        Axis::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub(crate) fn unit(&self) -> UnitVector<F> {
        UnitVector::new(self.x, self.y, self.z).scale(1.0 / self.norm())
    }
}

impl<K, F> std::ops::Sub for Vec3<K, F> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

/// Position and velocity in a single coordinate frame `F`.
///
/// `F` is one of the frame marker types (`Teme`, `Ecef`, `Enu`, `Lvlh`).
/// Frame-conversion methods are defined on the concrete type aliases
/// [`TemeState`], [`EcefState`], [`EnuState`] and [`LvlhState`] in the frames
/// module, so the compiler enforces correct frame usage at each conversion step.
///
/// [`TemeState`]: crate::TemeState
/// [`EcefState`]: crate::EcefState
/// [`EnuState`]: crate::EnuState
/// [`LvlhState`]: crate::LvlhState
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StateVector<F> {
    /// Position in frame `F`, in metres.
    pub position: Position<F>,
    /// Velocity in frame `F`, in metres per second.
    pub velocity: Velocity<F>,
}

impl<F> StateVector<F> {
    /// Pair a position and a velocity in the same frame.
    #[must_use]
    pub fn new(position: Position<F>, velocity: Velocity<F>) -> Self {
        Self { position, velocity }
    }

    /// Radial velocity: dot product of position and velocity (m²/s).
    ///
    /// Positive when the satellite is moving away from the origin, negative when approaching.
    /// Zero at apogee and perigee.
    #[must_use]
    pub fn radial_velocity(&self) -> f64 {
        self.position.dot(&self.velocity)
    }
}

/// Position vector in frame `F` (metres).
pub type Position<F> = Vec3<markers::Position, F>;

/// Velocity vector in frame `F` (metres per second).
pub type Velocity<F> = Vec3<markers::Velocity, F>;

/// Dimensionless unit vector in frame `F`.
///
/// A separate kind from [`Position`] so a direction and a displacement in
/// metres cannot be mixed at a function boundary.
pub type UnitVector<F> = Vec3<markers::Unit, F>;

/// Derived direction or axis in frame `F` (a cross product, an eccentricity
/// vector): not a displacement, a velocity or a normalised direction.
pub(crate) type Axis<F> = Vec3<markers::Axis, F>;

mod markers {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Axis;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Position;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Velocity;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Unit;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Convenience frame marker just for these tests — we only care about the
    // arithmetic, not which coordinate frame is in use.
    #[derive(Debug, Clone, Copy, Default)]
    struct TestFrame;

    type TestState = StateVector<TestFrame>;
    type TestPos = Position<TestFrame>;
    type TestVel = Velocity<TestFrame>;

    #[test]
    fn test_radial_velocity_away() {
        // Position and velocity point in the same direction → positive r·v
        let sv = TestState::new(
            TestPos::new(7_000_000.0, 0.0, 0.0),
            TestVel::new(7_000.0, 0.0, 0.0),
        );
        assert!(sv.radial_velocity() > 0.0);
    }

    #[test]
    fn test_radial_velocity_toward() {
        // Velocity opposes position → negative r·v (approaching)
        let sv = TestState::new(
            TestPos::new(7_000_000.0, 0.0, 0.0),
            TestVel::new(-7_000.0, 0.0, 0.0),
        );
        assert!(sv.radial_velocity() < 0.0);
    }

    #[test]
    fn test_radial_velocity_perpendicular() {
        // Velocity perpendicular to position → zero r·v (at apsis or circular orbit)
        let sv = TestState::new(
            TestPos::new(7_000_000.0, 0.0, 0.0),
            TestVel::new(0.0, 7_000.0, 0.0),
        );
        assert_eq!(sv.radial_velocity(), 0.0);
    }
}
