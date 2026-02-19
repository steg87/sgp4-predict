use std::marker::PhantomData;

/// A generic 3-component vector parameterised by kind `K` (position vs velocity)
/// and coordinate frame `F`. All vector logic lives here once.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec3<K, F> {
    pub x: f64,
    pub y: f64,
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

    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

impl<K, F> std::ops::Sub for Vec3<K, F> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

/// State vector (position + velocity) in frame `F`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateVector<F> {
    pub position: Position<F>,
    pub velocity: Velocity<F>,
}

impl<F> StateVector<F> {
    pub fn new(position: Position<F>, velocity: Velocity<F>) -> Self {
        Self { position, velocity }
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
