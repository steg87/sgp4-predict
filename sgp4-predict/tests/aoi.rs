mod common;

use chrono::{DateTime, Duration, Utc};
use sgp4_predict::{
    AoiIterOpts, Area, Circle, Coverage, Degrees, DetectError, Error, FillRule, LatLon, Polygon,
    Predictor, Rectangle, Refinement,
};

/// A ~6° × 7° box over Scotland — small enough that overpasses are short and
/// sparse, so the adaptive scan has to find them rather than stumble into them.
///
/// Its ground-track spacing means it is only overflown on some days, so tests
/// using it search a month; the dense-scan cross-checks use the larger areas
/// below over a single day, since brute-forcing a month at 1 s is far too slow.
fn scotland() -> Polygon {
    Polygon::new([
        LatLon {
            latitude: Degrees(54.0),
            longitude: Degrees(-8.0),
        },
        LatLon {
            latitude: Degrees(54.0),
            longitude: Degrees(-1.0),
        },
        LatLon {
            latitude: Degrees(60.0),
            longitude: Degrees(-1.0),
        },
        LatLon {
            latitude: Degrees(60.0),
            longitude: Degrees(-8.0),
        },
    ])
    .expect("valid box")
}

/// A wide box over Europe, crossed several times a day.
fn europe() -> Polygon {
    Polygon::new([
        (Degrees(40.0), Degrees(-10.0)),
        (Degrees(40.0), Degrees(30.0)),
        (Degrees(65.0), Degrees(30.0)),
        (Degrees(65.0), Degrees(-10.0)),
    ])
    .expect("valid box")
}

fn day() -> std::ops::Range<DateTime<Utc>> {
    common::datetime("2025-12-20T12:00:00Z")..common::datetime("2025-12-21T12:00:00Z")
}

fn month() -> std::ops::Range<DateTime<Utc>> {
    common::datetime("2025-12-20T12:00:00Z")..common::datetime("2026-01-21T12:00:00Z")
}

fn inside(p: &Predictor, area: &impl Area, t: DateTime<Utc>) -> bool {
    let point = p.sub_point(t).expect("propagation failed");
    area.signed_angular_offset(point.into()).to_f64() >= 0.0
}

/// Brute-force the windows by stepping one second at a time. Slow, but it
/// depends on nothing the adaptive scan does, so it is independent ground
/// truth for the tests below.
fn dense_scan(
    p: &Predictor,
    area: &impl Area,
    interval: std::ops::Range<DateTime<Utc>>,
    step: Duration,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    dense_scan_with(interval, step, |t| inside(p, area, t))
}

/// The same brute-force scan over an arbitrary predicate, for the cases where
/// "inside" means more than the offset's sign.
fn dense_scan_with(
    interval: std::ops::Range<DateTime<Utc>>,
    step: Duration,
    inside: impl Fn(DateTime<Utc>) -> bool,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let mut windows = Vec::new();
    let mut open: Option<DateTime<Utc>> = None;
    let mut t = interval.start;
    while t < interval.end {
        match (inside(t), open) {
            (true, None) => open = Some(t),
            (false, Some(start)) => {
                windows.push((start, t));
                open = None;
            }
            _ => {}
        }
        t += step;
    }
    if let Some(start) = open {
        windows.push((start, interval.end));
    }
    windows
}

#[test]
fn test_aoi_windows() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = scotland();

    let windows = p
        .aoi_iter(&area, month())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        !windows.is_empty(),
        "no overpasses over Scotland in a month"
    );

    for w in &windows {
        // The ground track crosses a ~700 km box at ~6.6 km/s, so a couple of
        // minutes at most; a corner clip can be arbitrarily brief. Measured in
        // milliseconds so a sub-second graze is short, not zero.
        let millis = (w.end - w.start).num_milliseconds();
        assert!(
            (1..=300_000).contains(&millis),
            "window duration {millis} ms is implausible for this box"
        );

        // Self-consistency, needing no external reference: the midpoint is
        // inside and both shoulders are outside.
        let mid = w.start + (w.end - w.start) / 2;
        assert!(inside(&p, &area, mid), "midpoint of {w:?} is not inside");
        assert!(
            !inside(&p, &area, w.start - Duration::seconds(5)),
            "5 s before {} is already inside",
            w.start
        );
        assert!(
            !inside(&p, &area, w.end + Duration::seconds(5)),
            "5 s after {} is still inside",
            w.end
        );
    }
}

/// The adaptive scan claims it cannot step over a crossing however narrow.
/// This is what actually tests that claim: a one-second brute-force scan must
/// find the same windows, at the same times.
#[test]
fn test_aoi_matches_dense_scan() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = europe();

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    assert_eq!(
        adaptive.len(),
        dense.len(),
        "adaptive found {} windows, dense scan found {}",
        adaptive.len(),
        dense.len()
    );

    for (a, (start, end)) in adaptive.iter().zip(&dense) {
        // The dense scan brackets each crossing to within its own 1 s step,
        // and the adaptive result is refined inside that bracket.
        assert!(
            (a.start - *start).num_milliseconds().abs() <= 1_000,
            "start {} vs dense {start}",
            a.start
        );
        assert!(
            (a.end - *end).num_milliseconds().abs() <= 1_000,
            "end {} vs dense {end}",
            a.end
        );
    }
}

/// The step strategy is only safe if the ground point never moves faster than
/// the bound derived from the element set. Check that against the propagator.
#[test]
fn test_ground_track_never_outruns_the_step_bound() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();

    // Mirrors `max_sub_point_rate`, which is private to the crate.
    const OMEGA_EARTH: f64 = 7.292_115_0e-5;
    const E2: f64 = (1.0 / 298.257_223_563) * (2.0 - 1.0 / 298.257_223_563);
    let n = 14.30821394 * std::f64::consts::TAU / 86_400.0;
    let e: f64 = 0.0001197;
    let bound =
        1.05 / (1.0 - E2) * (n * (1.0 - e * e).sqrt() / ((1.0 - e) * (1.0 - e)) + OMEGA_EARTH);

    let unit = |t| {
        let g: LatLon = p.sub_point(t).unwrap().into();
        let (sin_lat, cos_lat) = g.latitude.radians().sin_cos();
        let (sin_lon, cos_lon) = g.longitude.radians().sin_cos();
        [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat]
    };

    // Sampling resolution, not a tolerance: a longer step averages the rate
    // over the chord and would hide a short-lived peak on an eccentric orbit.
    let step = Duration::seconds(1);
    let mut t = day().start;
    let mut worst: f64 = 0.0;
    while t < day().end {
        let (a, b) = (unit(t), unit(t + step));
        let dot: f64 = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let rate = dot.clamp(-1.0, 1.0).acos() / step.num_seconds() as f64;
        worst = worst.max(rate);
        t += step;
    }

    assert!(
        worst < bound,
        "observed ground-track rate {worst:.3e} rad/s exceeds the bound {bound:.3e}"
    );
}

#[test]
fn test_aoi_opts_max_duration_exceeded() {
    // A real overpass is well over 1 second; capping max_window_duration that
    // low must surface as WindowTooLong rather than silently truncating.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = scotland();

    let result = p
        .aoi_iter_with_opts(
            &area,
            month(),
            AoiIterOpts {
                max_window_duration: Duration::seconds(1),
                ..AoiIterOpts::default()
            },
            Refinement::default(),
        )
        .collect::<Result<Vec<_>, _>>();

    assert!(matches!(
        result,
        Err(Error::Detect(DetectError::WindowTooLong { .. }))
    ));
}

#[test]
fn test_aoi_opts_zero_step_does_not_hang() {
    // A zero (or negative) step never advances the coarse scan or boundary
    // walk on its own; opts values must be floored to a positive duration
    // rather than stalling forever. Bracket a known entry tightly so the
    // interval starts outside the window and ends inside it, exercising both
    // the coarse-scan floor and the boundary-walk floor.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = scotland();

    let reference = p
        .aoi_iter(&area, month())
        .next()
        .expect("no overpass during search interval")
        .expect("error detecting overpass");

    let result = p
        .aoi_iter_with_opts(
            &area,
            (reference.start - Duration::seconds(10))..(reference.start + Duration::seconds(10)),
            AoiIterOpts {
                min_step: Duration::zero(),
                max_step: Duration::zero(),
                walk_step: Duration::zero(),
                ..AoiIterOpts::default()
            },
            Refinement::default(),
        )
        .collect::<Result<Vec<_>, _>>();

    assert!(
        result.is_ok(),
        "zero-step opts should not hang or error: {result:?}"
    );
}

#[test]
fn test_detect_aoi() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = scotland();

    let reference = p
        .aoi_iter(&area, month())
        .next()
        .expect("no overpass during search interval")
        .expect("error detecting overpass");
    let mid = reference.start + (reference.end - reference.start) / 2;

    let detected = p
        .detect_aoi(mid, &area)
        .expect("detection failed")
        .expect("a window is in progress at the midpoint");
    assert!(
        (detected.start - reference.start).num_milliseconds().abs() <= 1_000,
        "point query start {} vs iterator {}",
        detected.start,
        reference.start
    );
    assert!(
        (detected.end - reference.end).num_milliseconds().abs() <= 1_000,
        "point query end {} vs iterator {}",
        detected.end,
        reference.end
    );

    // Well clear of any window.
    assert!(
        p.detect_aoi(reference.start - Duration::minutes(20), &area)
            .expect("detection failed")
            .is_none()
    );
}

#[test]
fn test_aoi_window_start_inside_interval() {
    // A window already in progress when the search interval opens is excluded
    // by default, the same rule transits follow.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = scotland();

    let reference = p
        .aoi_iter(&area, month())
        .next()
        .expect("no overpass during search interval")
        .expect("error detecting overpass");
    let mid = reference.start + (reference.end - reference.start) / 2;

    let windows = p
        .aoi_iter(&area, mid..month().end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        windows.iter().all(|w| w.start >= mid),
        "a window already open at the interval start was returned"
    );
}

#[test]
fn test_antimeridian_polygon() {
    // Nothing about the detector special-cases ±180°, so this is really a
    // check that no longitude wrapping crept in.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Polygon::new([
        (Degrees(-20.0), Degrees(160.0)),
        (Degrees(-20.0), Degrees(-160.0)),
        (Degrees(20.0), Degrees(-160.0)),
        (Degrees(20.0), Degrees(160.0)),
    ])
    .expect("valid box");

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    assert!(!adaptive.is_empty(), "no equatorial overpasses in a day");
    assert_eq!(adaptive.len(), dense.len(), "adaptive and dense disagree");
}

#[test]
fn test_polar_polygon() {
    // A cap around the north pole. Sentinel-2C is sun-synchronous at ~98.6°
    // inclination, so it clips this on every orbit.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Polygon::new((0..36).map(|i| (Degrees(80.0), Degrees(i as f64 * 10.0 - 180.0))))
        .expect("valid cap");

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    // ~14.3 orbits a day, one polar crossing each.
    assert!(
        (13..=16).contains(&adaptive.len()),
        "expected roughly one crossing per orbit, got {}",
        adaptive.len()
    );
    assert_eq!(adaptive.len(), dense.len(), "adaptive and dense disagree");
}

#[test]
fn test_vertex_order_does_not_change_windows() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let forward = scotland();
    let reversed = Polygon::new(forward.vertices().rev().collect::<Vec<_>>()).expect("valid box");

    let a = p
        .aoi_iter(&forward, month())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let b = p
        .aoi_iter(&reversed, month())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        // Reversing the ring reorders the per-edge minimum, so the offsets
        // differ in the last bits and the refined crossings by nanoseconds.
        // Agreement to the solver's own 1 ms tolerance is the real bar.
        assert!((x.start - y.start).num_milliseconds().abs() <= 1);
        assert!((x.end - y.end).num_milliseconds().abs() <= 1);
    }
}

#[test]
fn test_concave_area_windows_are_split() {
    // An L-shaped area over Europe. The notch is what makes this worth
    // testing: a ground track can leave and re-enter, and the two windows must
    // stay separate rather than merging across the gap.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Polygon::new([
        (Degrees(40.0), Degrees(0.0)),
        (Degrees(40.0), Degrees(30.0)),
        (Degrees(55.0), Degrees(30.0)),
        (Degrees(55.0), Degrees(15.0)),
        (Degrees(70.0), Degrees(15.0)),
        (Degrees(70.0), Degrees(0.0)),
    ])
    .expect("valid L");

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    assert!(!adaptive.is_empty());
    assert_eq!(
        adaptive.len(),
        dense.len(),
        "concave area: adaptive found {} windows, dense scan {}",
        adaptive.len(),
        dense.len()
    );
}

#[test]
fn test_fill_rule_changes_windows_over_a_self_intersecting_area() {
    // A pentagram winds its centre twice, so NonZero fills it and EvenOdd
    // leaves a hole. Made large enough that the ground track crosses the
    // central pentagon, which is the only region the rules disagree on.
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let star: Vec<_> = (0..5)
        .map(|i| {
            let theta = (90.0 + 144.0 * i as f64).to_radians();
            (
                Degrees(50.0 + 25.0 * f64::sin(theta)),
                Degrees(25.0 * f64::cos(theta)),
            )
        })
        .collect();

    let nonzero = Polygon::new(star.clone()).expect("valid star");
    let evenodd = Polygon::new(star)
        .expect("valid star")
        .with_fill_rule(FillRule::EvenOdd);

    for area in [&nonzero, &evenodd] {
        let adaptive = p
            .aoi_iter(area, day())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let dense = dense_scan(&p, area, day(), Duration::seconds(1));
        assert_eq!(
            adaptive.len(),
            dense.len(),
            "self-intersecting area: adaptive and dense disagree"
        );
    }

    let total = |area: &Polygon| -> i64 {
        p.aoi_iter(area, day())
            .map(|w| w.unwrap())
            .map(|w| (w.end - w.start).num_seconds())
            .sum()
    };
    assert!(
        total(&nonzero) > total(&evenodd),
        "punching a hole in the middle must reduce total time over the area"
    );
}

#[test]
fn test_rectangle_matches_dense_scan() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Rectangle::new(
        LatLon {
            latitude: Degrees(40.0),
            longitude: Degrees(-10.0),
        },
        LatLon {
            latitude: Degrees(65.0),
            longitude: Degrees(30.0),
        },
    )
    .expect("valid box");

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    assert!(!adaptive.is_empty());
    assert_eq!(adaptive.len(), dense.len(), "adaptive and dense disagree");
    for (a, (start, end)) in adaptive.iter().zip(&dense) {
        assert!((a.start - *start).num_milliseconds().abs() <= 1_000);
        assert!((a.end - *end).num_milliseconds().abs() <= 1_000);
    }
}

/// A `Rectangle` honours its stated latitude bounds; the four-vertex `Polygon`
/// with the same corners does not.
///
/// A great circle between two points at equal latitude always bows toward the
/// nearer pole, so both of the polygon's horizontal edges shift north: the 65°N
/// edge to ~66.3°N and the 40°N edge to ~41.8°N. The region is displaced
/// poleward, not merely enlarged — it admits ground north of the stated bound
/// and excludes ground just inside the southern one.
#[test]
fn test_rectangle_honours_its_latitude_bounds_where_a_polygon_does_not() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let corners = [(40.0, -10.0), (40.0, 30.0), (65.0, 30.0), (65.0, -10.0)];

    let rect = Rectangle::new(
        (Degrees(corners[0].0), Degrees(corners[0].1)),
        (Degrees(corners[2].0), Degrees(corners[2].1)),
    )
    .expect("valid box");
    let poly =
        Polygon::new(corners.map(|(lat, lon)| (Degrees(lat), Degrees(lon)))).expect("valid ring");

    // Highest latitude the ground track reaches while a window is open.
    let peak_latitude = |windows: Vec<sgp4_predict::AoiWindow>| -> f64 {
        let mut peak = f64::NEG_INFINITY;
        for w in windows {
            let mut t = w.start;
            while t <= w.end {
                peak = peak.max(p.sub_point(t).unwrap().latitude.to_f64());
                t += Duration::seconds(1);
            }
        }
        peak
    };

    let rect_peak = peak_latitude(
        p.aoi_iter(&rect, day())
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );
    let poly_peak = peak_latitude(
        p.aoi_iter(&poly, day())
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    assert!(
        rect_peak <= 65.0 + 1e-3,
        "rectangle admitted latitude {rect_peak}, north of its 65° bound"
    );
    assert!(
        poly_peak > 65.5,
        "the polygon's edges are expected to bow past 65°, but peaked at {poly_peak}"
    );
}

#[test]
fn test_circle_matches_dense_scan() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Circle::new(
        LatLon {
            latitude: Degrees(52.0),
            longitude: Degrees(10.0),
        },
        Degrees(10.0),
    )
    .expect("valid circle");

    let adaptive = p
        .aoi_iter(&area, day())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let dense = dense_scan(&p, &area, day(), Duration::seconds(1));

    assert!(!adaptive.is_empty());
    assert_eq!(adaptive.len(), dense.len(), "adaptive and dense disagree");
    for (a, (start, end)) in adaptive.iter().zip(&dense) {
        assert!((a.start - *start).num_milliseconds().abs() <= 1_000);
        assert!((a.end - *end).num_milliseconds().abs() <= 1_000);
    }
}

#[test]
fn test_ground_track_iter_matches_sub_point() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let start = day().start;

    let track = p
        .ground_track_iter(start..start + Duration::minutes(10), Duration::minutes(1))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(track.len(), 10);
    for (t, point) in track {
        assert_eq!(point, p.sub_point(t).unwrap());
        assert!((-90.0..=90.0).contains(&point.latitude.to_f64()));
        assert!((-180.0..=180.0).contains(&point.longitude.to_f64()));
        // Sentinel-2C flies at ~790 km.
        assert!(
            (700_000.0..900_000.0).contains(&point.altitude),
            "altitude {} m is not a plausible Sentinel-2C orbit",
            point.altitude
        );
    }
}

fn off_nadir(degrees: f64, coverage: Coverage) -> AoiIterOpts {
    AoiIterOpts {
        max_off_nadir: Degrees(degrees).into(),
        coverage,
        ..AoiIterOpts::default()
    }
}

fn windows(
    p: &Predictor,
    area: &impl Area,
    opts: AoiIterOpts,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    p.aoi_iter_with_opts(area, day(), opts, Refinement::default())
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .map(|w| (w.start, w.end))
        .collect()
}

/// The default is nadir-only, which is what makes the field of regard additive
/// rather than a change in behaviour.
#[test]
fn test_zero_off_nadir_matches_the_default() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = europe();

    assert_eq!(
        windows(&p, &area, AoiIterOpts::default()),
        windows(&p, &area, off_nadir(0.0, Coverage::Any)),
    );
}

/// A wider field of regard reaches the area sooner and holds it longer, so
/// each nadir-only window must sit inside a wider one.
#[test]
fn test_wider_field_of_regard_contains_the_nadir_windows() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = europe();

    let nadir = windows(&p, &area, AoiIterOpts::default());
    let wide = windows(&p, &area, off_nadir(30.0, Coverage::Any));

    assert!(!nadir.is_empty());
    for (start, end) in &nadir {
        assert!(
            wide.iter().any(|(s, e)| s <= start && e >= end),
            "nadir window {start}..{end} is not contained in any 30° window"
        );
    }
    let span = |ws: &[(DateTime<Utc>, DateTime<Utc>)]| {
        ws.iter().map(|(s, e)| (*e - *s).num_seconds()).sum::<i64>()
    };
    assert!(span(&wide) > span(&nadir), "a 30° cone must see more");
}

/// The reach is the whole point: an area the ground track never enters is
/// still accessible from a wide enough field of regard.
#[test]
fn test_area_the_ground_track_misses_is_still_reachable() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    // Placed off the ground track, inside a 30° field of regard but outside a
    // 5° one.
    let area = Circle::new((Degrees(52.0), Degrees(10.0)), Degrees(1.0)).expect("valid circle");

    let narrow = windows(&p, &area, off_nadir(5.0, Coverage::Any));
    let wide = windows(&p, &area, off_nadir(45.0, Coverage::Any));

    assert!(
        wide.len() > narrow.len(),
        "a wider cone must find more windows"
    );
}

/// Requiring the whole area is strictly harder than requiring part of it.
#[test]
fn test_full_coverage_is_contained_in_any_coverage() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Circle::new((Degrees(52.0), Degrees(10.0)), Degrees(2.0)).expect("valid circle");

    let any = windows(&p, &area, off_nadir(45.0, Coverage::Any));
    let full = windows(&p, &area, off_nadir(45.0, Coverage::Full));

    assert!(!full.is_empty(), "the area should fit inside a 45° cone");
    for (start, end) in &full {
        assert!(
            any.iter().any(|(s, e)| s <= start && e >= end),
            "full-coverage window {start}..{end} escapes every any-coverage window"
        );
    }
}

/// An area wider than the field of regard can never be covered entirely, even
/// while parts of it are always in reach.
#[test]
fn test_area_wider_than_the_cone_is_never_fully_covered() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Circle::new((Degrees(52.0), Degrees(10.0)), Degrees(30.0)).expect("valid circle");

    assert!(!windows(&p, &area, off_nadir(10.0, Coverage::Any)).is_empty());
    assert!(
        windows(&p, &area, off_nadir(10.0, Coverage::Full)).is_empty(),
        "a 60°-wide area cannot fit inside a 10° field of regard"
    );
}

/// The adaptive scan's no-skip guarantee has to survive the shifted threshold,
/// so cross-check it against a brute-force scan as the nadir-only path is.
#[test]
fn test_off_nadir_matches_dense_scan() {
    let p = Predictor::from_tle(common::create_tle()).unwrap();
    let area = Circle::new((Degrees(52.0), Degrees(10.0)), Degrees(4.0)).expect("valid circle");
    let opts = off_nadir(30.0, Coverage::Any);

    let adaptive = windows(&p, &area, opts);
    let dense = dense_scan_with(day(), Duration::seconds(1), |t| {
        let point = p.sub_point(t).expect("propagation failed");
        // Mirrors `AreaInView`, whose internals are private to the crate.
        let reach = reach_at(&p, t, 30.0);
        area.signed_angular_offset(point.into()).to_f64() + reach >= 0.0
    });

    assert!(!adaptive.is_empty());
    assert_eq!(adaptive.len(), dense.len(), "adaptive and dense disagree");
    for ((a_start, a_end), (start, end)) in adaptive.iter().zip(&dense) {
        assert!((*a_start - *start).num_milliseconds().abs() <= 1_000);
        assert!((*a_end - *end).num_milliseconds().abs() <= 1_000);
    }
}

/// The central angle a payload at `off_nadir_deg` reaches, recomputed from the
/// propagated state so the test shares no code with the implementation.
fn reach_at(p: &Predictor, t: DateTime<Utc>, off_nadir_deg: f64) -> f64 {
    let point = p.sub_point(t).expect("propagation failed");
    let state = p.propagate(t).expect("propagation failed").to_ecef(t);
    let pos = state.position;
    let r = (pos.x * pos.x + pos.y * pos.y + pos.z * pos.z).sqrt();
    let re = r - point.altitude;
    let eta = Degrees(off_nadir_deg).radians();
    let horizon = (re / r).acos();
    let s = (r / re) * eta.sin();
    if s >= 1.0 {
        horizon
    } else {
        (s.asin() - eta).min(horizon)
    }
}
