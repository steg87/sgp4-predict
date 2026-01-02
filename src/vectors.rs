use crate::units::{self, SI};

/// State vector, takes frame as generic
#[derive(Debug, Clone, Copy, Default)]
pub struct StateVector<F> {
    pub position: Position,
    pub velocity: Velocity,
    _frame: std::marker::PhantomData<F>,
}

impl<F> StateVector<F> {
    pub fn new(position: Position, velocity: Velocity) -> Self {
        Self {
            position,
            velocity,
            _frame: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Position {
    pub x: units::Length,
    pub y: units::Length,
    pub z: units::Length,
}

impl std::ops::Sub for Position {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_si(
            self.x.to_si() - rhs.x.to_si(),
            self.y.to_si() - rhs.y.to_si(),
            self.z.to_si() - rhs.z.to_si(),
        )
    }
}

/// Velocity vector
#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity {
    pub x: units::Velocity,
    pub y: units::Velocity,
    pub z: units::Velocity,
}

impl std::ops::Sub for Velocity {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_si(
            self.x.to_si() - rhs.x.to_si(),
            self.y.to_si() - rhs.y.to_si(),
            self.z.to_si() - rhs.z.to_si(),
        )
    }
}
