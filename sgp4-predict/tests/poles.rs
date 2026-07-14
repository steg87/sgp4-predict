mod common;

use chrono::Duration;
use sgp4_predict::{PoleEvent, Predictor};

#[test]
fn test_pole_approaches() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();

    // Two orbital periods for Sentinel-2C (~100 min/orbit → ~200 min)
    let start = common::datetime("2025-12-20T12:00:00Z");
    let end = start + Duration::minutes(200);

    let approaches = p
        .pole_approach_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(
        !approaches.is_empty(),
        "expected at least one pole approach"
    );

    // Verify that events strictly alternate
    for window in approaches.windows(2) {
        assert_ne!(
            window[0].event, window[1].event,
            "consecutive pole approaches must alternate between North and South"
        );
    }

    // Verify both event types are present
    assert!(
        approaches.iter().any(|a| a.event == PoleEvent::North),
        "expected at least one northern approach"
    );
    assert!(
        approaches.iter().any(|a| a.event == PoleEvent::South),
        "expected at least one southern approach"
    );

    // Sentinel-2C has inclination 98.5671°, a retrograde near-polar orbit, so the
    // maximum geocentric latitude reached is ≈ 180° − 98.5671° = 81.43°.
    for approach in &approaches {
        let expected = 81.43;
        let sign = match approach.event {
            PoleEvent::North => 1.0,
            PoleEvent::South => -1.0,
        };
        assert!(
            (approach.latitude_deg() - sign * expected).abs() < 1.0,
            "{:?} latitude {:.3}° is outside expected ±{expected}° range",
            approach.event,
            approach.latitude_deg()
        );
    }

    // Each detected approach must be a local extremum: |z| at the event time should
    // exceed |z| at ±10 s.
    let offset = Duration::seconds(10);
    for approach in &approaches {
        let z_at = |t| p.propagate(t).unwrap().position.z;
        let z = z_at(approach.time);
        let z_before = z_at(approach.time - offset);
        let z_after = z_at(approach.time + offset);
        match approach.event {
            PoleEvent::North => assert!(
                z > z_before && z > z_after,
                "north approach z={z:.1} m is not a local maximum (before={z_before:.1}, after={z_after:.1})"
            ),
            PoleEvent::South => assert!(
                z < z_before && z < z_after,
                "south approach z={z:.1} m is not a local minimum (before={z_before:.1}, after={z_after:.1})"
            ),
        }
    }
}
