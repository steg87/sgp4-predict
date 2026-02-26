//! Generic vector types with compile-time coordinate-frame tracking.
//!
//! [`Position`] and [`Velocity`] are type aliases over [`Vec3`], distinguished
//! by kind markers so they cannot be accidentally mixed. [`StateVector`] pairs
//! the two and carries a phantom frame type `F` — one of the TEME, ECEF, or
//! ENU marker structs — so the compiler rejects passing a vector in the wrong
//! frame.
//!
//! All values are in SI units: metres for position, metres per second for
//! velocity.

use std::marker::PhantomData;

/// Backing 3-component vector type, parameterised by kind `K` and frame `F`.
///
/// Not used directly; prefer the [`Position`] and [`Velocity`] type aliases.
#[derive(Debug, Clone, Copy, Default)]
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
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            _marker: PhantomData,
        }
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
/// `F` is one of the frame marker types (`Teme`, `Ecef`, `Enu`).
/// Frame-conversion methods are defined on the concrete type aliases
/// [`TemeState`], [`EcefState`], and [`EnuState`] in the frames module,
/// so the compiler enforces correct frame usage at each conversion step.
///
/// [`TemeState`]: crate::TemeState
#[derive(Debug, Clone, Copy, Default)]
pub struct StateVector<F> {
    pub position: Position<F>,
    pub velocity: Velocity<F>,
}

impl<F> StateVector<F> {
    pub fn new(position: Position<F>, velocity: Velocity<F>) -> Self {
        Self { position, velocity }
    }

    /// Radial velocity: dot product of position and velocity (m²/s).
    ///
    /// Positive when the satellite is moving away from the origin, negative when approaching.
    /// Zero at apogee and perigee.
    pub fn radial_velocity(&self) -> f64 {
        self.position.x * self.velocity.x
            + self.position.y * self.velocity.y
            + self.position.z * self.velocity.z
    }
}

/// Position vector in frame `F` (metres).
pub type Position<F> = Vec3<markers::Position, F>;

/// Velocity vector in frame `F` (metres per second).
pub type Velocity<F> = Vec3<markers::Velocity, F>;

mod markers {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Position;

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Velocity;
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
