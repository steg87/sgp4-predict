mod common;

use chrono::Duration;
use sgp4_predict::{ArgLatEvent, Degrees, Predictor};

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

// Mean elements of `common::create_tle()`: inclination, argument of perigee
// and mean anomaly (degrees), and mean motion (rev/day). With e ≈ 1e-4 the
// mean argument of latitude at epoch is ω + M, advancing at the mean motion.
// Osculating and mean values differ by a few tenths of a degree, so these
// checks are independent of the vector algebra but only good to that scale.
const INCLINATION: f64 = 98.5671;
const U_EPOCH: f64 = (95.1447 + 264.9872) % 360.0;
const DEG_PER_MIN: f64 = 14.308_213_94 * 360.0 / 1440.0;

#[test]
fn test_event_time_matches_mean_elements() {
    let (p, _) = run(0.0);
    let u0 = 100.0;
    let first = p
        .arg_lat_iter(p.epoch()..p.epoch() + Duration::minutes(60), Degrees(u0))
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(first.event, ArgLatEvent::Rising);
    let expected = (u0 - U_EPOCH) / DEG_PER_MIN * 60.0;
    let got = (first.time - p.epoch()).num_milliseconds() as f64 / 1e3;
    assert!((got - expected).abs() < 30.0, "{got} s vs {expected} s");
}

#[test]
fn test_latitude_at_events_follows_inclination() {
    // sin(latitude) = sin(i)·sin(u): +φ at u₀, −φ at u₀ + 180°.
    let (p, events) = run(30.0);
    assert!(!events.is_empty());
    let want = (INCLINATION.to_radians().sin() * 30f64.to_radians().sin()).asin();
    for e in &events {
        let s = p.propagate(e.time).unwrap();
        let r = (s.position.x.powi(2) + s.position.y.powi(2) + s.position.z.powi(2)).sqrt();
        let lat = (s.position.z / r).asin().to_degrees();
        let want = match e.event {
            ArgLatEvent::Rising => want.to_degrees(),
            ArgLatEvent::Falling => -want.to_degrees(),
        };
        assert!((lat - want).abs() < 0.5, "{:?}: {lat}° vs {want}°", e.event);
    }
}

#[test]
fn test_argument_of_latitude_at_epoch() {
    let (p, _) = run(0.0);
    let u = p.argument_of_latitude(p.epoch()).unwrap().degrees();
    let diff = (u - U_EPOCH + 180.0).rem_euclid(360.0) - 180.0;
    assert!(diff.abs() < 0.5, "u = {u}°");
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
