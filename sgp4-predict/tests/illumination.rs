mod common;

use chrono::Duration;
use sgp4_predict::{IlluminationState, Predictor};

// Sentinel-2C orbital period is ~100 min.  Two orbits gives enough windows to
// exercise both sunlit and eclipse states and several full boundary crossings.
const TWO_ORBITS_MINS: i64 = 200;

// Plausible eclipse duration bounds for LEO (cylindrical shadow model).
// Sentinel-2C at ~786 km: eclipse is roughly 35 min per orbit; allow generous
// margin for the partial windows at the interval boundaries.
const MIN_ECLIPSE_SECS: i64 = 900; //  15 min
const MAX_ECLIPSE_SECS: i64 = 2700; //  45 min

fn predictor() -> Predictor {
    Predictor::new(&common::create_tle()).unwrap()
}

fn interval() -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    let start = common::datetime("2025-12-20T12:00:00Z");
    (start, start + Duration::minutes(TWO_ORBITS_MINS))
}

// ---------------------------------------------------------------------------
// Structural tests
// ---------------------------------------------------------------------------

/// Windows must exactly tile the search interval: no gaps, no overlaps.
#[test]
fn test_illumination_covers_interval() {
    let p = predictor();
    let (start, end) = interval();

    let windows: Vec<_> = p
        .illumination_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(!windows.is_empty(), "expected at least one window");

    assert_eq!(
        windows.first().unwrap().start,
        start,
        "first window must start at interval start"
    );
    assert_eq!(
        windows.last().unwrap().end,
        end,
        "last window must end at interval end"
    );

    for pair in windows.windows(2) {
        assert_eq!(
            pair[0].end, pair[1].start,
            "gap or overlap between consecutive windows: {:?} .. {:?}",
            pair[0].end, pair[1].start
        );
    }
}

/// Consecutive windows must alternate between Sunlit and Eclipse.
#[test]
fn test_illumination_alternates() {
    let p = predictor();
    let (start, end) = interval();

    let windows: Vec<_> = p
        .illumination_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for pair in windows.windows(2) {
        assert_ne!(
            pair[0].state, pair[1].state,
            "consecutive windows must not have the same state (boundary at {:?})",
            pair[0].end
        );
    }
}

/// Both Sunlit and Eclipse windows must appear over two orbital periods.
#[test]
fn test_illumination_both_states_present() {
    let p = predictor();
    let (start, end) = interval();

    let windows: Vec<_> = p
        .illumination_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(
        windows.iter().any(|w| w.state == IlluminationState::Sunlit),
        "no sunlit window found over two orbital periods"
    );
    assert!(
        windows
            .iter()
            .any(|w| w.state == IlluminationState::Eclipse),
        "no eclipse window found over two orbital periods"
    );
}

/// `illumination_state()` at the midpoint of every window must agree with the
/// window state returned by the iterator.
#[test]
fn test_illumination_state_consistent() {
    let p = predictor();
    let (start, end) = interval();

    let windows: Vec<_> = p
        .illumination_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for window in &windows {
        // Skip zero-duration windows (only possible if an interval boundary
        // coincides exactly with a shadow crossing).
        if window.start == window.end {
            continue;
        }
        let mid = window.start + (window.end - window.start) / 2;
        let state = p.illumination_state(mid).unwrap();
        assert_eq!(
            state, window.state,
            "illumination_state at {mid} disagrees with window state {:?} (window {:.0?}–{:.0?})",
            window.state, window.start, window.end,
        );
    }
}

/// Interior eclipse windows (not clipped by the interval boundary) must fall
/// within the plausible duration range for a LEO satellite.
#[test]
fn test_illumination_eclipse_duration_plausible() {
    let p = predictor();
    let (start, end) = interval();

    let windows: Vec<_> = p
        .illumination_iter(start..end)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Skip the first and last windows: they may be partial due to clamping.
    let interior = match windows.len() {
        0 | 1 => return,
        _ => &windows[1..windows.len() - 1],
    };

    for window in interior
        .iter()
        .filter(|w| w.state == IlluminationState::Eclipse)
    {
        let secs = (window.end - window.start).num_seconds();
        assert!(
            (MIN_ECLIPSE_SECS..=MAX_ECLIPSE_SECS).contains(&secs),
            "eclipse duration {secs}s is outside expected range \
             [{MIN_ECLIPSE_SECS}, {MAX_ECLIPSE_SECS}]s  \
             (window {:.0?}–{:.0?})",
            window.start,
            window.end,
        );
    }
}

#[test]
#[ignore]
fn test_illumination_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite.clone();
    let p = Predictor::new(&tle).unwrap();

    let start = p.epoch();
    let end = start + Duration::days(3);

    let start_str = start.format("%Y%m%dT%H%M%S").to_string();
    let end_str = end.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_illumination_{}_{}.csv", sat_id, start_str, end_str);

    std::fs::create_dir_all("tests/results").unwrap();
    let filepath = format!("tests/results/{}", filename);

    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(file, "start,end,illumination,duration").unwrap();

    let mut count = 0;
    for illumination in p.illumination_iter(start..end) {
        let illumination = illumination.unwrap();
        let event_str = match illumination.state {
            IlluminationState::Sunlit => "sunlit",
            IlluminationState::Eclipse => "eclipse",
        };
        let duration = illumination.end - illumination.start;
        let duration_str = humantime::format_duration(std::time::Duration::from_secs_f32(
            duration.as_seconds_f32().round(),
        ))
        .to_string();
        writeln!(
            file,
            "{},{},{},{}",
            illumination.start.format("%Y-%m-%d %H:%M:%S"),
            illumination.end.format("%Y-%m-%d %H:%M:%S"),
            event_str,
            duration_str
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} illumination windows to {}", count, filepath);
}
