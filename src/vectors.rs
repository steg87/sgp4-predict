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
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl std::ops::Sub for Position {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

/// Velocity vector
#[derive(Debug, Clone, Copy, Default)]
pub struct Velocity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl std::ops::Sub for Velocity {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}
