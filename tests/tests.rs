use chrono::{DateTime, Duration, Utc};

use sgp4_predict::{HasId, HasTle, Observer, Predictor};

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

struct GroundStation {
    latitude_deg: f64,
    longitude_deg: f64,
    altitude: f64,
}

impl GroundStation {
    fn new(latitude_deg: f64, longitude_deg: f64, altitude: f64) -> Self {
        Self {
            latitude_deg,
            longitude_deg,
            altitude,
        }
    }
}

impl Observer for GroundStation {
    fn latitude(&self) -> f64 {
        self.latitude_deg.to_radians()
    }
    fn longitude(&self) -> f64 {
        self.longitude_deg.to_radians()
    }
    fn altitude(&self) -> f64 {
        self.altitude
    }
}

fn create_tle() -> Tle {
    Tle {
        satellite: "SENTINEL-2C".to_string(),
        line_1: "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990".to_string(),
        line_2: "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740".to_string(),
    }
}

fn datetime(dt: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(dt)
        .unwrap()
        .with_timezone(&Utc)
}

fn angle(theta: f64) -> f64 {
    theta.to_radians()
}

#[test]
fn test_propagate() {
    let tle = create_tle();
    let p = Predictor::new(&tle);
    let transits = p
        .prediction_iter(
            &(datetime("2025-12-20T12:00:00Z")..datetime("2025-12-23T12:00:00Z")),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!transits.is_empty());
}

#[test]
fn test_observe() {
    let tle = create_tle();
    let p = Predictor::new(&tle);
    let gs = GroundStation::new(55.8642, -4.2518, 40.0);
    let observations = p
        .observation_iter(
            &gs,
            &(datetime("2025-12-20T12:00:00Z")..datetime("2025-12-23T12:00:00Z")),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!observations.is_empty());
}

#[test]
fn test_transits() {
    let tle = create_tle();
    let p = Predictor::new(&tle);
    let gs = GroundStation::new(55.8642, -4.2518, 40.0);
    let transits = p
        .transits_iter(
            &gs,
            datetime("2025-12-20T12:00:00Z")..datetime("2026-01-21T12:00:00Z"),
            angle(0.0),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!transits.is_empty());
}

#[test]
#[ignore]
fn test_transits_to_csv() {
    use std::io::Write;

    let tle = create_tle();
    let sat_id = tle.id();
    let p = Predictor::new(&tle);
    let gs = GroundStation::new(55.8642, -4.2518, 40.0);

    let start_dt = datetime("2025-12-20T12:00:00Z");
    let end_dt = datetime("2026-01-21T12:00:00Z");

    // Format filename: {sat_id}_transits_{start_dt}_{end_dt}.csv
    let start_str = start_dt.format("%Y%m%dT%H%M%S").to_string();
    let end_str = end_dt.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_transits_{}_{}.csv", sat_id, start_str, end_str);

    // Create results directory if it doesn't exist
    std::fs::create_dir_all("tests/results").unwrap();
    let filepath = format!("tests/results/{}", filename);

    // Write transits to CSV file
    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(file, "start,end,aos_azimuth_deg,los_azimuth_deg,duration").unwrap();

    let mut count = 0;
    for transit in p.transits_iter(&gs, start_dt..end_dt, angle(0.0)) {
        let transit = transit.unwrap();

        // Get observations at start and end to extract azimuth
        let obs_start = p.observe_at(transit.start, &gs).unwrap();
        let obs_end = p.observe_at(transit.end, &gs).unwrap();

        // Calculate duration
        let duration = transit.end - transit.start;
        let duration_str = humantime::format_duration(std::time::Duration::from_secs_f32(
            duration.as_seconds_f32(),
        ))
        .to_string();

        // Convert azimuth to degrees
        let aos_az_deg = obs_start.azimuth.to_degrees();
        let los_az_deg = obs_end.azimuth.to_degrees();

        writeln!(
            file,
            "{},{},{:.2},{:.2},{}",
            transit.start.format("%Y-%m-%d %H:%M:%S"),
            transit.end.format("%Y-%m-%d %H:%M:%S"),
            aos_az_deg,
            los_az_deg,
            duration_str
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} transits to {}", count, filepath);
}

#[test]
#[ignore]
fn test_next_transit_observations_to_csv() {
    use std::io::Write;

    let tle = create_tle();
    let sat_id = tle.id();
    let p = Predictor::new(&tle);
    let gs = GroundStation::new(55.8642, -4.2518, 40.0);

    // Calculate transits over the next 3 hours
    let start_dt = datetime("2025-12-20T12:00:00Z");
    let end_dt = start_dt + Duration::hours(3);

    let next_transit = p
        .transits_iter(&gs, start_dt..end_dt, angle(0.0))
        .next()
        .expect("No transits found in the next 3 hours")
        .unwrap();

    println!(
        "First transit: {} to {}",
        next_transit.start.format("%Y-%m-%d %H:%M:%S"),
        next_transit.end.format("%Y-%m-%d %H:%M:%S")
    );

    // Create results directory if it doesn't exist
    std::fs::create_dir_all("tests/results").unwrap();

    // Format filename
    let transit_start_str = next_transit.start.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_observations_{}.csv", sat_id, transit_start_str);
    let filepath = format!("tests/results/{}", filename);

    // Write observations to CSV file
    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(
        file,
        "time,azimuth_deg,elevation_deg,range_km,range_rate_km_s"
    )
    .unwrap();

    let mut count = 0;
    for obs in p
        .observation_iter(&gs, &next_transit, Duration::seconds(10))
        .chain(std::iter::once(Ok((
            next_transit.end,
            p.observe_at(next_transit.end, &gs).unwrap(),
        ))))
    {
        let (time, obs) = obs.unwrap();

        // Convert to degrees and km
        let az_deg = obs.azimuth.to_degrees();
        let el_deg = obs.elevation.to_degrees();
        let range_km = obs.range / 1000.0;
        let range_rate_km_s = obs.range_rate / 1000.0;

        writeln!(
            file,
            "{},{:.2},{:.2},{:.2},{:.4}",
            time.format("%Y-%m-%d %H:%M:%S"),
            az_deg,
            el_deg,
            range_km,
            range_rate_km_s
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} observations to {}", count, filepath);
}
