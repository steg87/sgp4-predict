#[cfg(feature = "uom")]
use uom::si::{angle::radian, length::meter, velocity::meter_per_second};

#[cfg(not(feature = "uom"))]
pub type Length = f64;
#[cfg(not(feature = "uom"))]
pub type Velocity = f64;
#[cfg(not(feature = "uom"))]
pub type Angle = f64;

#[cfg(feature = "uom")]
pub type Length = uom::si::f64::Length;
#[cfg(feature = "uom")]
pub type Velocity = uom::si::f64::Velocity;
#[cfg(feature = "uom")]
pub type Angle = uom::si::f64::Angle;

pub trait SI {
    fn to_si(&self) -> f64;
}
#[cfg(not(feature = "uom"))]
impl SI for f64 {
    #[inline]
    fn to_si(&self) -> f64 {
        *self
    }
}
#[cfg(feature = "uom")]
impl SI for uom::si::f64::Length {
    #[inline]
    fn to_si(&self) -> f64 {
        self.get::<meter>()
    }
}
#[cfg(feature = "uom")]
impl SI for uom::si::f64::Velocity {
    #[inline]
    fn to_si(&self) -> f64 {
        self.get::<meter_per_second>()
    }
}
#[cfg(feature = "uom")]
impl SI for uom::si::f64::Angle {
    #[inline]
    fn to_si(&self) -> f64 {
        self.get::<radian>()
    }
}

impl crate::Position {
    pub(crate) fn from_si(x: f64, y: f64, z: f64) -> Self {
        #[cfg(not(feature = "uom"))]
        return Self { x, y, z };
        #[cfg(feature = "uom")]
        return Self {
            x: uom::si::f64::Length::new::<meter>(x),
            y: uom::si::f64::Length::new::<meter>(y),
            z: uom::si::f64::Length::new::<meter>(z),
        };
    }
}

impl crate::Velocity {
    pub(crate) fn from_si(x: f64, y: f64, z: f64) -> Self {
        #[cfg(not(feature = "uom"))]
        return Self { x, y, z };
        #[cfg(feature = "uom")]
        return Self {
            x: uom::si::f64::Velocity::new::<meter_per_second>(x),
            y: uom::si::f64::Velocity::new::<meter_per_second>(y),
            z: uom::si::f64::Velocity::new::<meter_per_second>(z),
        };
    }
}

impl crate::Observation {
    pub(crate) fn from_si(azimuth: f64, elevation: f64, range: f64) -> Self {
        #[cfg(not(feature = "uom"))]
        return Self {
            azimuth,
            elevation,
            range,
        };
        #[cfg(feature = "uom")]
        return Self {
            azimuth: uom::si::f64::Angle::new::<radian>(azimuth),
            elevation: uom::si::f64::Angle::new::<radian>(elevation),
            range: uom::si::f64::Length::new::<meter>(range),
        };
    }
}
