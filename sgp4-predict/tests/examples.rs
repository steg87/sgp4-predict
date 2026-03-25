mod common;

use chrono::Duration;
use sgp4_predict::{IlluminationState, Predictor, Transit};

/// Check if a transit is in progress at a given time, and determine that start and end bounds if
/// so.
#[test]
fn detect_current_transit() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle).unwrap();
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);

    let start = p.epoch();
    let end = start + Duration::days(1);

    // Find the next transit so we have a known "mid-pass" time to hand to detect_transit.
    let transit: Transit = p
        .transits_iter(&gs, start..end, 5.0)
        .next()
        .expect("no transits during search interval")
        .expect("error calculating transit");

    // Simulate receiving the satellite 1/3 into the pass.
    let now = transit.start + (transit.end - transit.start) / 3;

    // Recover the full pass window from a single timestamp.
    let detected = p
        .detect_transit(now, &gs, 5.0)
        .expect("propagation error")
        .expect("satellite is not overhead at the given time");

    println!(
        "Pass window: {} → {} ({} s)",
        detected.start.format("%Y-%m-%d %H:%M:%S"),
        detected.end.format("%Y-%m-%d %H:%M:%S"),
        (detected.end - detected.start).num_seconds(),
    );

    // Sample observations across the detected pass so we can see the full arc.
    println!("time,azimuth [deg],elevation [deg],range [km],range rate [km/s]");
    for observation in p
        .observation_iter(&gs, detected, Duration::seconds(10))
        .include_end()
    {
        let (t, obs) = observation.expect("error calculating observation");
        println!(
            "{},{:.2},{:.2},{:.3},{:.3}",
            t.format("%Y-%m-%d %H:%M:%S"),
            obs.azimuth.to_degrees(),
            obs.elevation.to_degrees(),
            obs.range / 1000.0,
            obs.range_rate / 1000.0,
        );
    }
}

/// Find the next ground station pass above 15° that is fully sunlit and sample the observations
/// for it, including the end time.
#[test]
fn next_ground_station_pass_observations() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle).unwrap();
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);

    let start = p.epoch();
    let end = start + Duration::days(1);

    // Lazily find the next transit that satisfies the predicate
    let next_transit = p
        .transits_iter(&gs, start..end, 15.0)
        .find(|transit| {
            match transit {
                Ok(transit) => {
                    // Assume if transit start and end both sunlit then entire transit is
                    // sunlit. Skip this transit if either call fails.
                    matches!(
                        p.illumination_state(transit.start),
                        Ok(IlluminationState::Sunlit)
                    ) && matches!(
                        p.illumination_state(transit.end),
                        Ok(IlluminationState::Sunlit)
                    )
                }
                Err(_) => false, // Skip transits that could not be calculated
            }
        })
        .expect("no transits during search interval") // Iterator returned None
        .expect("error calculating transit"); // Redundant in this case since we've checked already

    // Calculate observations for the transit
    println!("time,azimuth [deg], elevation [deg], range [km], range rate [km/s]");
    for observation in p
        // Transit implements IntervalRange, so we can pass it as arg to observation_iter as interval
        .observation_iter(&gs, next_transit, Duration::seconds(10))
        // Include the transit end time in the output
        .include_end()
    {
        // Unwrap and shadow observation
        let (t, observation) = observation.expect("error calculating observation");
        // Output results to stdout
        println!(
            "{},{:.2},{:.2},{:.3},{:.3}",
            t.format("%Y-%m-%d %H:%M:%S"),
            observation.azimuth.to_degrees(),
            observation.elevation.to_degrees(),
            observation.range / 1000.0,
            observation.range_rate / 1000.0
        )
    }
}
