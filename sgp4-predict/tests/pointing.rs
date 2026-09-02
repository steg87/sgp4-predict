mod common;

use chrono::Duration;
use sgp4_predict::{Degrees, GeodeticPoint, Predictor};

fn predictor() -> Predictor {
    Predictor::from_tle(common::create_tle()).unwrap()
}

fn glasgow() -> GeodeticPoint {
    GeodeticPoint {
        latitude: Degrees(55.8642),
        longitude: Degrees(-4.2518),
        altitude: 40.0,
    }
}

/// Mid-pass over Glasgow, found once and reused by the cross-check tests.
fn overhead_time() -> chrono::DateTime<chrono::Utc> {
    let p = predictor();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let transit = p
        .transits_iter(glasgow(), start..start + Duration::days(2), Degrees(20.0))
        .next()
        .expect("Sentinel-2C passes over Glasgow within two days")
        .unwrap();
    p.max_elevation(transit, glasgow()).unwrap().0
}

// --- EcefState::to_teme ---

#[test]
fn test_to_teme_round_trips_to_ecef() {
    // The inverse must undo both the GMST rotation and the frame-drag term.
    // A non-J2000 epoch keeps GMST well away from zero.
    let t = common::datetime("2025-12-20T17:43:11Z");
    let teme = predictor().propagate(t).unwrap();
    let back = teme.to_ecef(t).to_teme(t);

    for (got, want, axis) in [
        (back.position.x, teme.position.x, "x"),
        (back.position.y, teme.position.y, "y"),
        (back.position.z, teme.position.z, "z"),
    ] {
        assert!(
            (got - want).abs() < 1e-6,
            "position {axis}: {got} != {want}"
        );
    }
    for (got, want, axis) in [
        (back.velocity.x, teme.velocity.x, "x"),
        (back.velocity.y, teme.velocity.y, "y"),
        (back.velocity.z, teme.velocity.z, "z"),
    ] {
        assert!(
            (got - want).abs() < 1e-9,
            "velocity {axis}: {got} != {want}"
        );
    }
}

// --- the LVLH triad ---

#[test]
fn test_lvlh_axes_are_orthonormal_and_right_handed() {
    // Probe the triad by pointing at three targets offset along each axis:
    // the returned direction *is* the axis expressed in LVLH, so the three
    // together are the identity matrix if and only if the triad is correct.
    let t = common::datetime("2025-12-20T12:00:00Z");
    let p = predictor();
    let sat = p.propagate(t).unwrap();

    // A target one metre along each TEME axis from the satellite.
    let axes: Vec<[f64; 3]> = (0..3)
        .map(|i| {
            let mut d = [0.0; 3];
            d[i] = 1.0;
            let target = sgp4_predict::TemeState::new(
                sgp4_predict::Position::new(
                    sat.position.x + d[0],
                    sat.position.y + d[1],
                    sat.position.z + d[2],
                ),
                sgp4_predict::Velocity::new(0.0, 0.0, 0.0),
            );
            let dir = sat.to_lvlh(target).to_pointing().direction;
            [dir.x, dir.y, dir.z]
        })
        .collect();

    // Each row is a unit vector, and distinct rows are orthogonal: that is
    // exactly the statement that the LVLH basis is orthonormal.
    for (i, a) in axes.iter().enumerate() {
        let norm = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        assert!((norm - 1.0).abs() < 1e-12, "row {i} norm {norm}");
        for (j, b) in axes.iter().enumerate().skip(i + 1) {
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            assert!(dot.abs() < 1e-12, "rows {i},{j} not orthogonal: {dot}");
        }
    }

    // Z must be exactly -r̂, which is what a velocity-first construction gets
    // wrong: it would tilt Z off nadir by the flight-path angle. Earth's
    // centre lies on the nadir ray by definition, so it probes Z directly.
    let centre = sgp4_predict::TemeState::new(
        sgp4_predict::Position::new(0.0, 0.0, 0.0),
        sgp4_predict::Velocity::new(0.0, 0.0, 0.0),
    );
    let d = sat.to_lvlh(centre).to_pointing().direction;
    assert!(
        d.x.abs() < 1e-12 && d.y.abs() < 1e-12,
        "nadir not +Z: {d:?}"
    );
    assert!((d.z - 1.0).abs() < 1e-12, "nadir z = {}", d.z);
}

// --- Predictor::point_at ---

#[test]
fn test_ground_point_beneath_is_almost_but_not_exactly_nadir() {
    // The sub-satellite point is *geodetic* — the ellipsoid normal through the
    // satellite — while nadir here is geocentric. The two differ by the
    // deflection of the vertical, under 0.19° of tilt anywhere on the
    // ellipsoid. Pins the documented convention: a zero here would mean nadir
    // had silently become normal-based.
    let t = common::datetime("2025-12-20T12:00:00Z");
    let p = predictor();
    let sub = p.sub_point(t).unwrap();
    let beneath = GeodeticPoint {
        latitude: sub.latitude,
        longitude: sub.longitude,
        altitude: 0.0,
    };
    let off_nadir = p.point_at(t, beneath).unwrap().off_nadir().degrees();

    assert!(
        off_nadir > 0.0,
        "geodetic sub-point is not geocentric nadir"
    );
    assert!(
        off_nadir < 0.19,
        "deflection of the vertical is {off_nadir}°"
    );
}

#[test]
fn test_target_ahead_of_the_satellite_is_along_track() {
    // The satellite's own position a few seconds later lies ahead along-track
    // and in the orbital plane, so it must sit on +X with no cross-track (Y)
    // component. Pins X's sign against the velocity direction.
    //
    // The *ground* track is not a substitute: Earth rotates under it, so a
    // later sub-point carries a real cross-track offset.
    let t = common::datetime("2025-12-20T12:00:00Z");
    let p = predictor();
    let sat = p.propagate(t).unwrap();
    let ahead = p.propagate(t + Duration::seconds(10)).unwrap();
    let d = sat.to_lvlh(ahead).to_pointing().direction;

    assert!(d.x > 0.0, "target ahead should be +X, got {}", d.x);
    assert!(
        d.y.abs() < 1e-6,
        "an in-plane target has no cross-track component, got {}",
        d.y
    );
}

#[test]
fn test_direction_is_a_unit_vector() {
    let t = overhead_time();
    let d = predictor().point_at(t, glasgow()).unwrap().direction;
    let norm = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    assert!((norm - 1.0).abs() < 1e-12, "norm {norm}");
}

#[test]
fn test_range_and_range_rate_match_the_observation() {
    // Range and range rate are the same two scalars seen from either end of
    // the link, in different frames. Range rate is the sharper of the two: a
    // ground point is stationary in ECEF and moving in TEME, so agreement here
    // is what proves `to_teme`'s frame-drag term.
    let t = overhead_time();
    let p = predictor();
    let obs = p.observe_at(t, glasgow()).unwrap();
    let pointing = p.point_at(t, glasgow()).unwrap();

    assert!(
        (pointing.range - obs.range).abs() < 1e-6,
        "range {} vs {}",
        pointing.range,
        obs.range
    );
    assert!(
        (pointing.range_rate - obs.range_rate).abs() < 1e-6,
        "range rate {} vs {}",
        pointing.range_rate,
        obs.range_rate
    );
}

#[test]
fn test_off_nadir_matches_elevation_on_the_equator() {
    // In the triangle centre-target-satellite, sin η = (rₑ/r)·cos ε. It holds
    // exactly only where the ellipsoid normal is radial, because `elevation`
    // is measured from the geodetic horizon while nadir is geocentric — so the
    // equator is where the two conventions can be compared without a fudge.
    let p = predictor();
    let target = GeodeticPoint {
        latitude: Degrees(0.0),
        longitude: Degrees(0.0),
        altitude: 0.0,
    };
    let start = common::datetime("2025-12-20T00:00:00Z");
    let transit = p
        .transits_iter(target, start..start + Duration::days(3), Degrees(30.0))
        .next()
        .expect("a high equatorial pass within three days")
        .unwrap();
    let t = p.max_elevation(transit, target).unwrap().0;

    let obs = p.observe_at(t, target).unwrap();
    let sat = p.propagate(t).unwrap().to_ecef(t);
    let r = (sat.position.x.powi(2) + sat.position.y.powi(2) + sat.position.z.powi(2)).sqrt();
    let e = target.to_ecef();
    let re = (e.position.x.powi(2) + e.position.y.powi(2) + e.position.z.powi(2)).sqrt();

    let expected = (re / r) * obs.elevation.to_f64().cos();
    let actual = p.point_at(t, target).unwrap().off_nadir().to_f64().sin();
    assert!(
        (actual - expected).abs() < 1e-12,
        "sin(off_nadir) = {actual}, expected (rₑ/r)·cos(elevation) = {expected}"
    );
}

#[test]
fn test_off_nadir_differs_from_elevation_only_by_the_vertical_deflection() {
    // Away from the equator the same relation is off by the deflection of the
    // vertical, and by no more. Pins the size of the documented mismatch so a
    // frame slip cannot hide inside it.
    let t = overhead_time();
    let p = predictor();
    let obs = p.observe_at(t, glasgow()).unwrap();
    let sat = p.propagate(t).unwrap().to_ecef(t);
    let r = (sat.position.x.powi(2) + sat.position.y.powi(2) + sat.position.z.powi(2)).sqrt();
    let e = glasgow().to_ecef();
    let re = (e.position.x.powi(2) + e.position.y.powi(2) + e.position.z.powi(2)).sqrt();

    let expected = (re / r) * obs.elevation.to_f64().cos();
    let actual = p.point_at(t, glasgow()).unwrap().off_nadir().to_f64().sin();
    let diff = (actual - expected).abs();

    assert!(diff > 1e-9, "at 55.9°N the two conventions must differ");
    assert!(
        diff < 5e-3,
        "deflection of the vertical is bounded, got {diff}"
    );
}

#[test]
fn test_zero_range_does_not_produce_nan() {
    // A target at the satellite has no direction. Must be finite, not NaN.
    let t = common::datetime("2025-12-20T12:00:00Z");
    let p = predictor();
    let sat = p.propagate(t).unwrap();
    let pointing = sat.to_lvlh(sat).to_pointing();

    assert!(pointing.direction.x.is_finite());
    assert!(pointing.direction.y.is_finite());
    assert!(pointing.direction.z.is_finite());
    assert!(pointing.range.is_finite());
    assert!(pointing.range_rate.is_finite());
    assert!(pointing.off_nadir().to_f64().is_finite());
}
