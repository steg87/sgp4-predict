#![allow(dead_code)]

use chrono::{DateTime, Utc};
use sgp4_predict::{HasId, HasTle, Observer};

pub struct Tle {
    pub satellite: String,
    pub line_1: String,
    pub line_2: String,
}

impl HasId for Tle {
    fn id(&self) -> &str {
        &self.satellite
    }
}

impl HasTle for Tle {
    fn line_1(&self) -> &str {
        &self.line_1
    }
    fn line_2(&self) -> &str {
        &self.line_2
    }
}

pub struct GroundStation {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude: f64,
}

impl GroundStation {
    pub fn new(latitude_deg: f64, longitude_deg: f64, altitude: f64) -> Self {
        Self {
            latitude_deg,
            longitude_deg,
            altitude,
        }
    }
}

impl Observer for GroundStation {
    fn latitude_deg(&self) -> f64 {
        self.latitude_deg
    }
    fn longitude_deg(&self) -> f64 {
        self.longitude_deg
    }
    fn altitude(&self) -> f64 {
        self.altitude
    }
}

pub fn load_tle_from_file(path: &std::path::Path) -> Tle {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read TLE file {}: {}", path.display(), e));
    let mut lines = content.lines();
    Tle {
        satellite: lines.next().unwrap().trim().to_string(),
        line_1: lines.next().unwrap().trim().to_string(),
        line_2: lines.next().unwrap().trim().to_string(),
    }
}

pub fn create_tle() -> Tle {
    Tle {
        satellite: "SENTINEL-2C".to_string(),
        line_1: "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990".to_string(),
        line_2: "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740".to_string(),
    }
}

pub fn datetime(dt: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(dt)
        .unwrap()
        .with_timezone(&Utc)
}
