mod common;

use chrono::Duration;
use sgp4_predict::Predictor;

const WGS84_A: f64 = 6_378_137.0; // metres

#[test]
fn test_propagate() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle).unwrap();

    // Sentinel-2C mean altitude is ~788 km (sun-synchronous orbit). J2 short-period
    // perturbations can shift the instantaneous geocentric radius by up to ~12 km,
    // so we allow ±15 km around the mean.
    let epoch_state = p.propagate(p.epoch()).unwrap();
    let pos = epoch_state.position;
    let altitude_m = (pos.x * pos.x + pos.y * pos.y + pos.z * pos.z).sqrt() - WGS84_A;
    assert!(
        (773_000.0..=803_000.0).contains(&altitude_m),
        "altitude at epoch {:.1} m is outside expected 788 km ± 15 km range",
        altitude_m
    );

    let start = p.epoch();
    let end = start + Duration::days(3);

    let predictions = p
        .prediction_iter(start..end, Duration::minutes(1))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!predictions.is_empty());
}
