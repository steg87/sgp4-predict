mod common;

use chrono::Duration;
use sgp4_predict::{ArgLatEvent, Degrees, Predictor};

/// Osculating argument of latitude in degrees, in [0, 360).
fn argument_of_latitude(p: &Predictor, t: chrono::DateTime<chrono::Utc>) -> f64 {
    let s = p.propagate(t).unwrap();
    let (r, v) = (s.position, s.velocity);
    let h = [
        r.y * v.z - r.z * v.y,
        r.z * v.x - r.x * v.z,
        r.x * v.y - r.y * v.x,
    ];
    let n = [-h[1], h[0], 0.0];
    let cross = [
        h[1] * n[2] - h[2] * n[1],
        h[2] * n[0] - h[0] * n[2],
        h[0] * n[1] - h[1] * n[0],
    ];
    let dot = |a: [f64; 3]| r.x * a[0] + r.y * a[1] + r.z * a[2];
    let norm = |a: [f64; 3]| a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let u = (dot(cross) / norm(cross))
        .atan2(dot(n) / norm(n))
        .to_degrees();
    u.rem_euclid(360.0)
}

fn run(argument: f64) -> (Predictor, Vec<sgp4_predict::ArgLatCrossing>) {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let events = p
        .arg_lat_iter(start..start + Duration::minutes(300), Degrees(argument))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (p, events)
}

#[test]
fn test_nodes_alternate_and_lie_on_the_equator() {
    let (p, events) = run(0.0);
    assert!(events.len() >= 5, "expected ~6 nodes in 300 min");
    for pair in events.windows(2) {
        assert_ne!(pair[0].event, pair[1].event);
    }
    for e in &events {
        let s = p.propagate(e.time).unwrap();
        assert!(s.position.z.abs() < 50.0, "z = {} m", s.position.z);
        assert_eq!(e.event == ArgLatEvent::Rising, s.velocity.z > 0.0);
    }
}

#[test]
fn test_argument_events_hit_u0_and_opposite() {
    let (p, events) = run(30.0);
    assert!(!events.is_empty());
    for e in &events {
        let want = match e.event {
            ArgLatEvent::Rising => 30.0,
            ArgLatEvent::Falling => 210.0,
        };
        let got = argument_of_latitude(&p, e.time);
        assert!((got - want).abs() < 1e-4, "{:?}: u = {got}", e.event);
    }
}

#[test]
fn test_non_finite_argument_errors() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let first = p
        .arg_lat_iter(start..start + Duration::minutes(10), Degrees(f64::NAN))
        .next();
    assert!(matches!(first, Some(Err(_))));
}
