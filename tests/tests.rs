use chrono::{DateTime, Duration, Utc};

use sgp4_predict::{HasId, HasTle, Predictor};

struct Tle {
    satellite: String,
    line_1: String,
    line_2: String,
}

impl HasId for Tle {
    fn id(&self) -> String {
        self.satellite.clone()
    }
}

impl HasTle for Tle {
    fn line_1(&self) -> String {
        self.line_1.clone()
    }
    fn line_2(&self) -> String {
        self.line_2.clone()
    }
}

fn create_tle() -> Tle {
    Tle {
        satellite: "GALILEO A".to_string(),
        line_1: "1 67160U 25302A   25352.14605887 -.00000007  00000+0  00000+0 0  9994".to_string(),
        line_2: "2 67160  54.2406 107.2544 0008423 237.6458 122.2317  1.72906186    28".to_string(),
    }
}

fn datetime(dt: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(dt)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn test_propagate() {
    let tle = create_tle();
    let p = Predictor::new(&tle);
    let transits = p
        .propagate(
            datetime("2025-12-20T12:00:00Z")..datetime("2025-12-23T12:00:00Z"),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(transits.len() > 0);
}
