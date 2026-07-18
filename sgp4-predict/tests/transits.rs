mod common;

use chrono::Duration;
use sgp4_predict::{GroundObserver, Predictor, Refinement};

#[test]
fn test_transits() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(55.8642, -4.2518, 40.0);
    let transits = p
        .transits_iter(
            &gs,
            common::datetime("2025-12-20T12:00:00Z")..common::datetime("2026-01-21T12:00:00Z"),
            0.0,
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!transits.is_empty());

    // A nadir pass of Sentinel-2C (~788 km) from Glasgow is at most ~905 s long.
    // Allow up to 960 s (16 min) for headroom; minimum 60 s to reject false detections.
    for transit in &transits {
        let duration_secs = (transit.end - transit.start).num_seconds();
        assert!(
            (60..=960).contains(&duration_secs),
            "transit duration {} s is outside expected [1 min, 16 min] range",
            duration_secs
        );
    }
}

#[test]
fn test_transit_start_inside_interval() {
    // Design decision: a transit already in progress when the search window opens is
    // excluded. Only transits whose AOS falls within the window are returned.
    let tle = common::create_tle();
    // This test compares two independent refinements of the same crossing to
    // within 1 ms, so it needs a tighter tolerance than the 1 ms default.
    let p = Predictor::from_tle(&tle)
        .unwrap()
        .with_refinement(Refinement {
            time_tolerance: 1e-4,
            ..Refinement::default()
        });
    let gs = GroundObserver::new(55.8642, -4.2518, 40.0);

    // Find the first two transits over a wide window.
    let wide_start = common::datetime("2025-12-20T12:00:00Z");
    let wide_end = common::datetime("2026-01-21T12:00:00Z");
    let all_transits = p
        .transits_iter(&gs, wide_start..wide_end, 0.0)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        all_transits.len() >= 2,
        "need at least two transits for this test"
    );

    let first = &all_transits[0];
    let second = &all_transits[1];

    // Open a new search window mid-way through the first transit.
    let mid_first = first.start + (first.end - first.start) / 2;
    let trimmed = p
        .transits_iter(&gs, mid_first..wide_end, 0.0)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // The in-progress transit must be absent; the next complete transit must be first.
    assert!(
        !trimmed.is_empty(),
        "expected at least one transit after mid-transit window start"
    );
    assert!(
        trimmed[0].start >= first.end,
        "in-progress transit was returned; its start {:?} should be >= first transit end {:?}",
        trimmed[0].start,
        first.end
    );
    // The AoS of the next transit should match the wide-search result to within 1ms.
    // Root-finding starts from a different bracket, so the refinement may converge
    // to a slightly different value even for the same crossing.
    let start_diff = (trimmed[0].start - second.start).num_milliseconds().abs();
    assert!(
        start_diff < 1,
        "first returned transit start {:?} differs from wide-search second transit start {:?} by {} ms",
        trimmed[0].start,
        second.start,
        start_diff
    );
}

#[test]
fn test_detect_transit() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(55.8642, -4.2518, 40.0);

    // Find the first transit via the iterator (ground truth).
    let transits = p
        .transits_iter(
            &gs,
            common::datetime("2025-12-20T12:00:00Z")..common::datetime("2026-01-21T12:00:00Z"),
            0.0,
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        !transits.is_empty(),
        "need at least one transit for this test"
    );

    let reference = &transits[0];
    let midpoint = reference.start + (reference.end - reference.start) / 2;

    // detect_transit at the midpoint must return Some and match the iterator result to ~1 s.
    let detected = p.detect_transit(midpoint, &gs, 0.0).unwrap();
    let detected = detected.expect("expected Some(Transit) at midpoint of a known pass");

    let start_diff = (detected.start - reference.start).num_milliseconds().abs();
    let end_diff = (detected.end - reference.end).num_milliseconds().abs();
    assert!(
        start_diff <= 1000,
        "detected start {:?} differs from reference {:?} by {} ms",
        detected.start,
        reference.start,
        start_diff
    );
    assert!(
        end_diff <= 1000,
        "detected end {:?} differs from reference {:?} by {} ms",
        detected.end,
        reference.end,
        end_diff
    );

    // detect_transit at a time clearly outside any transit must return None.
    // Use a time 5 minutes before the first transit's AoS.
    let before_transit = reference.start - Duration::minutes(5);
    let outside = p.detect_transit(before_transit, &gs, 0.0).unwrap();
    assert!(
        outside.is_none(),
        "expected None before any transit, got {:?}",
        outside
    );
}

#[test]
fn test_max_elevation_trimmed_interval_returns_higher_endpoint() {
    // Design decision: if the interval is trimmed short of the true peak
    // (e.g. a partial transit), max_elevation returns the higher-elevation
    // endpoint instead of erroring.
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(55.8642, -4.2518, 40.0);

    let transits = p
        .transits_iter(
            &gs,
            common::datetime("2025-12-20T12:00:00Z")..common::datetime("2026-01-21T12:00:00Z"),
            0.0,
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let (transit, tca_time) = transits
        .iter()
        .find_map(|t| {
            let (tca_time, _) = p.max_elevation(*t, &gs).unwrap();
            (tca_time - t.start > Duration::seconds(30) && t.end - tca_time > Duration::seconds(30))
                .then_some((*t, tca_time))
        })
        .expect("need a transit with margin on both sides of its peak");

    // Trimmed to end well before the peak: elevation is still rising
    // throughout, so no falling crossing exists and the trimmed end (the
    // higher-elevation endpoint) must be returned.
    let before_peak = transit.start..(tca_time - Duration::seconds(20));
    let (t, obs) = p.max_elevation(before_peak.clone(), &gs).unwrap();
    assert_eq!(t, before_peak.end);
    let start_obs = p.observe_at(before_peak.start, &gs).unwrap();
    assert!(obs.elevation > start_obs.elevation);

    // Trimmed to start well after the peak: elevation is falling
    // throughout, so the trimmed start (the higher-elevation endpoint)
    // must be returned.
    let after_peak = (tca_time + Duration::seconds(20))..transit.end;
    let (t, obs) = p.max_elevation(after_peak.clone(), &gs).unwrap();
    assert_eq!(t, after_peak.start);
    let end_obs = p.observe_at(after_peak.end, &gs).unwrap();
    assert!(obs.elevation > end_obs.elevation);
}

#[test]
#[ignore]
fn test_transits_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite_name.clone();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(55.8642, -4.2518, 40.0);

    let start = p.epoch();
    let end = start + Duration::days(3);

    let start_str = start.format("%Y%m%dT%H%M%S").to_string();
    let end_str = end.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_transits_{}_{}.csv", sat_id, start_str, end_str);

    std::fs::create_dir_all("tests/results").unwrap();
    let filepath = format!("tests/results/{}", filename);

    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(
        file,
        "start,end,aos_azimuth_deg,los_azimuth_deg,tca_elevation_deg,duration"
    )
    .unwrap();

    let mut count = 0;
    for transit in p.transits_iter(&gs, start..end, 0.0) {
        let transit = transit.unwrap();

        let obs_start = p.observe_at(transit.start, &gs).unwrap();
        let obs_end = p.observe_at(transit.end, &gs).unwrap();
        let (_, obs_tca) = p.max_elevation(transit, &gs).unwrap();

        let duration = transit.end - transit.start;
        let duration_str = humantime::format_duration(std::time::Duration::from_secs_f32(
            duration.as_seconds_f32().round(),
        ))
        .to_string();

        writeln!(
            file,
            "{},{},{:.2},{:.2},{:.2},{}",
            transit.start.format("%Y-%m-%d %H:%M:%S"),
            transit.end.format("%Y-%m-%d %H:%M:%S"),
            obs_start.azimuth.to_degrees(),
            obs_end.azimuth.to_degrees(),
            obs_tca.elevation.to_degrees(),
            duration_str
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} transits to {}", count, filepath);
}
