mod common;

use chrono::Duration;
use sgp4_predict::{Degrees, GeodeticPoint, Predictor};

#[test]
fn test_observe() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GeodeticPoint {
        latitude: Degrees(55.8642),
        longitude: Degrees(-4.2518),
        altitude: 40.0,
    };
    let observations = p
        .observation_iter(
            gs,
            common::datetime("2025-12-20T12:00:00Z")..common::datetime("2025-12-23T12:00:00Z"),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!observations.is_empty());
}

/// Cross-validate a single `observe_at` result against skyfield 1.49 (Python).
///
/// Reference values were computed with:
///   sat = EarthSatellite(line1, line2, "SENTINEL-2C", ts)
///   observer = wgs84.latlon(55.8642, -4.2518, elevation_m=40.0)
///   t = ts.utc(2025, 12, 20, 12, 35, 0)
///   alt, az, dist = (sat - observer).at(t).altaz()
///
/// Both implementations use the same SGP4 propagator; differences arise only
/// from GMST and ENU projection details. Observed agreement: az 0.002°, el 0.001°, range 2 m.
#[test]
fn test_observe_cross_validate_skyfield() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GeodeticPoint {
        latitude: Degrees(55.8642),
        longitude: Degrees(-4.2518),
        altitude: 40.0,
    };

    let t = common::datetime("2025-12-20T12:35:00Z");
    let obs = p.observe_at(t, gs).unwrap();

    let az_deg = obs.azimuth.normalized().degrees();
    let el_deg = obs.elevation.degrees();
    let range_km = obs.range / 1_000.0;

    // skyfield reference (computed offline)
    let ref_az_deg = 311.314_513_67_f64;
    let ref_el_deg = 37.581_643_93_f64;
    let ref_range_km = 1_204.652_907_f64;

    assert!(
        (az_deg - ref_az_deg).abs() < 0.01,
        "azimuth {:.6}° differs from skyfield reference {:.6}° by more than 0.01°",
        az_deg,
        ref_az_deg
    );
    assert!(
        (el_deg - ref_el_deg).abs() < 0.01,
        "elevation {:.6}° differs from skyfield reference {:.6}° by more than 0.01°",
        el_deg,
        ref_el_deg
    );
    assert!(
        (range_km - ref_range_km).abs() < 0.1,
        "range {:.3} km differs from skyfield reference {:.3} km by more than 100 m",
        range_km,
        ref_range_km
    );
}

#[test]
#[ignore]
fn test_next_transit_observations_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite_name.clone();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GeodeticPoint {
        latitude: Degrees(55.8642),
        longitude: Degrees(-4.2518),
        altitude: 40.0,
    };

    let start = p.epoch();
    let end = start + Duration::hours(3);

    let next_transit = p
        .transits_iter(gs, start..end, Degrees(0.0))
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
    writeln!(
        file,
        "time,azimuth_deg,elevation_deg,range_km,range_rate_km_s"
    )
    .unwrap();

    let mut count = 0;
    for obs in p
        .observation_iter(gs, next_transit, Duration::seconds(10))
        .chain(std::iter::once(Ok((
            next_transit.end,
            p.observe_at(next_transit.end, gs).unwrap(),
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
