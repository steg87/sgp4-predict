#![allow(dead_code)]

use chrono::{DateTime, Utc};
pub use sgp4_predict::Tle;

pub fn create_tle() -> Tle {
    Tle::new(
        "SENTINEL-2C",
        "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990",
        "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740",
    )
}

pub fn load_tle_from_file(path: &std::path::Path) -> Tle {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read TLE file {}: {}", path.display(), e));
    content
        .parse()
        .unwrap_or_else(|e| panic!("failed to parse TLE file {}: {e}", path.display()))
}

pub fn datetime(dt: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(dt)
        .unwrap()
        .with_timezone(&Utc)
}
