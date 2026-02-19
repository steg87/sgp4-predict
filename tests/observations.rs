mod common;

use chrono::Duration;
use sgp4_predict::Predictor;

#[test]
fn test_observe() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle);
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);
    let observations = p
        .observation_iter(
            &gs,
            &(common::datetime("2025-12-20T12:00:00Z")..common::datetime("2025-12-23T12:00:00Z")),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!observations.is_empty());
}

#[test]
#[ignore]
fn test_next_transit_observations_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite.clone();
    let p = Predictor::new(&tle);
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);

    let start_dt = common::datetime("2025-12-20T12:00:00Z");
    let end_dt = start_dt + Duration::hours(3);

    let next_transit = p
        .transits_iter(&gs, start_dt..end_dt, 0.0)
        .next()
        .expect("No transits found in the next 3 hours")
        .unwrap();

    println!(
        "First transit: {} to {}",
        next_transit.start.format("%Y-%m-%d %H:%M:%S"),
        next_transit.end.format("%Y-%m-%d %H:%M:%S")
    );

    std::fs::create_dir_all("tests/results").unwrap();

    let transit_start_str = next_transit.start.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_observations_{}.csv", sat_id, transit_start_str);
    let filepath = format!("tests/results/{}", filename);

    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(file, "time,azimuth_deg,elevation_deg,range_km,range_rate_km_s").unwrap();

    let mut count = 0;
    for obs in p
        .observation_iter(&gs, &next_transit, Duration::seconds(10))
        .chain(std::iter::once(Ok((
            next_transit.end,
            p.observe_at(next_transit.end, &gs).unwrap(),
        ))))
    {
        let (time, obs) = obs.unwrap();
        writeln!(
            file,
            "{},{:.2},{:.2},{:.2},{:.4}",
            time.format("%Y-%m-%d %H:%M:%S"),
            obs.azimuth.to_degrees(),
            obs.elevation.to_degrees(),
            obs.range / 1000.0,
            obs.range_rate / 1000.0,
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} observations to {}", count, filepath);
}
