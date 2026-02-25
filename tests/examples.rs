mod common;

use chrono::Duration;
use sgp4_predict::{IlluminationState, Predictor};

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
        .transits_iter(&gs, start..end, f64::to_radians(15.0))
        .find(|transit| {
            match transit {
                Ok(transit) => {
                    // Assume if transit start and end both sunlit then entire transit is
                    // sunlit. Skip this transit if either call fails.
                    matches!(p.illumination_state(transit.start), Ok(IlluminationState::Sunlit))
                        && matches!(p.illumination_state(transit.end), Ok(IlluminationState::Sunlit))
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
