mod common;

use chrono::Duration;
use sgp4_predict::{
    ApsisEvent, Error, KeplerianElements, Position, Predictor, TemeState, Velocity,
};
use std::f64::consts::{PI, TAU};

fn predictor() -> Predictor {
    Predictor::new(common::create_tle()).unwrap()
}

#[test]
fn test_osculating_matches_mean_at_epoch() {
    let p = predictor();
    let k = p.osculating_elements(p.epoch()).unwrap();
    let m = p.mean_elements();
    assert!((k.inclination.0 - m.inclination.0).abs().to_degrees() < 0.1);
    assert!((k.period - m.period).num_seconds().abs() < 10);
    assert!(k.eccentricity < 1e-2);
    // Sun-synchronous LEO: ~800 km.
    assert!((700e3..900e3).contains(&k.perigee_altitude));
}

#[test]
fn test_argument_of_perigee_plus_anomaly_is_argument_of_latitude() {
    let p = predictor();
    let t = p.epoch() + Duration::minutes(37);
    let k = p.osculating_elements(t).unwrap();
    let u = (k.argument_of_perigee.0 + k.true_anomaly.0).rem_euclid(TAU);
    let expected = p.argument_of_latitude(t).unwrap().0;
    let diff = (u - expected + PI).rem_euclid(TAU) - PI;
    assert!(diff.abs() < 1e-9, "diff = {diff}");
}

#[test]
fn test_apsis_altitudes_match_apsis_events() {
    let p = predictor();
    let t = p.epoch();
    let k = p.osculating_elements(t).unwrap();
    for apsis in p
        .apsis_iter(t..t + Duration::minutes(120))
        .map(Result::unwrap)
    {
        let expected = match apsis.event {
            ApsisEvent::Perigee => k.perigee_altitude,
            ApsisEvent::Apogee => k.apogee_altitude,
        };
        // Osculating and propagated apsides differ by J2 periodics.
        assert!((apsis.altitude - expected).abs() < 20e3);
    }
}

#[test]
fn test_mean_elements_match_tle_text() {
    let m = predictor().mean_elements();
    assert!((m.inclination.degrees() - 98.5671).abs() < 1e-9);
    assert!((m.raan.degrees() - 69.0082).abs() < 1e-9);
    assert!((m.eccentricity - 0.0001197).abs() < 1e-12);
    assert!((m.mean_motion - 14.308_213_94 * TAU / 86_400.0).abs() < 1e-12);
    assert!((m.period.num_seconds() - 6038).abs() <= 1);
    assert!((m.bstar - 7.0244e-5).abs() < 1e-12);
}

fn state(r: [f64; 3], v: [f64; 3]) -> TemeState {
    TemeState::new(
        Position::new(r[0], r[1], r[2]),
        Velocity::new(v[0], v[1], v[2]),
    )
}

#[test]
fn test_circular_equatorial_conventions() {
    let r = 7_000_000.0;
    let g = sgp4::WGS84;
    let mu = g.ke * g.ke * g.ae.powi(3) * 1e9 / 3600.0;
    let v = (mu / r).sqrt();
    // Prograde, at 90° from +X.
    let k = state([0.0, r, 0.0], [-v, 0.0, 0.0]).keplerian().unwrap();
    assert_eq!(k.raan.0, 0.0);
    assert_eq!(k.argument_of_perigee.0, 0.0);
    assert!(k.inclination.0 < 1e-9);
    assert!((k.true_anomaly.0 - PI / 2.0).abs() < 1e-6);
    // Retrograde at the same place: angle runs against the motion of +Y.
    let k = state([0.0, r, 0.0], [v, 0.0, 0.0]).keplerian().unwrap();
    assert!((k.inclination.0 - PI).abs() < 1e-9);
    assert!((k.true_anomaly.0 - 3.0 * PI / 2.0).abs() < 1e-6);
}

#[test]
fn test_unbound_state_is_rejected() {
    let s = state([7_000_000.0, 0.0, 0.0], [0.0, 12_000.0, 0.0]);
    assert_eq!(s.keplerian().unwrap_err(), Error::NotBound);
}

fn mu() -> f64 {
    let g = sgp4::WGS84;
    g.ke * g.ke * g.ae.powi(3) * 1e9 / 3600.0
}

/// Rebuild r and v from the elements: perifocal state rotated by
/// R3(-Ω) R1(-i) R3(-ω).
fn from_elements(k: &KeplerianElements) -> ([f64; 3], [f64; 3]) {
    let (e, nu) = (k.eccentricity, k.true_anomaly.0);
    let p = k.semi_major_axis * (1.0 - e * e);
    let r = p / (1.0 + e * nu.cos());
    let k_v = (mu() / p).sqrt();
    let rotate = |x: f64, y: f64| {
        let (so, co) = k.argument_of_perigee.0.sin_cos();
        let (si, ci) = k.inclination.0.sin_cos();
        let (sn, cn) = k.raan.0.sin_cos();
        [
            (cn * co - sn * so * ci) * x + (-cn * so - sn * co * ci) * y,
            (sn * co + cn * so * ci) * x + (-sn * so + cn * co * ci) * y,
            so * si * x + co * si * y,
        ]
    };
    (
        rotate(r * nu.cos(), r * nu.sin()),
        rotate(-k_v * nu.sin(), k_v * (e + nu.cos())),
    )
}

fn assert_round_trip(s: &TemeState) {
    let (r, v) = from_elements(&s.keplerian().unwrap());
    let r0 = [s.position.x, s.position.y, s.position.z];
    let v0 = [s.velocity.x, s.velocity.y, s.velocity.z];
    for i in 0..3 {
        assert!((r[i] - r0[i]).abs() < 1e-3, "r[{i}]: {} vs {}", r[i], r0[i]);
        assert!((v[i] - v0[i]).abs() < 1e-6, "v[{i}]: {} vs {}", v[i], v0[i]);
    }
}

#[test]
fn test_elements_round_trip_to_the_state() {
    let p = predictor();
    for minutes in [0, 17, 41, 73, 97] {
        assert_round_trip(&p.propagate(p.epoch() + Duration::minutes(minutes)).unwrap());
    }
}

#[test]
fn test_elements_round_trip_for_an_eccentric_inclined_orbit() {
    // Molniya-like: exercises a non-zero argument of perigee and a node in
    // each quadrant, which the near-circular TLE does not.
    for (r, v) in [
        ([-3.0e6, 5.0e6, 4.0e6], [-6_000.0, -4_000.0, 5_000.0]),
        ([4.0e6, -6.0e6, -2.0e6], [5_500.0, 3_000.0, -4_500.0]),
    ] {
        assert_round_trip(&state(r, v));
    }
}
