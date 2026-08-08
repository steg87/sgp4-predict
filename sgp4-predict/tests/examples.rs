mod common;

use chrono::{DateTime, Duration, Utc};
use sgp4_predict::{
    Degrees, FallibleIter, GroundObserver, IlluminationState, IntervalRange, Observation,
    Predictor, Transit,
};

/// Propagate the satellite state in TEME and ECEF frames for the next day, sampled every 15 minutes.
#[test]
fn daily_state_vectors() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();

    let start = p.epoch();
    let end = start + Duration::days(1);

    println!("time,x [km],y [km],z [km],vx [km/s],vy [km/s],vz [km/s]");
    for (t, teme) in p
        .prediction_iter(start..end, Duration::minutes(15))
        .skip_errors()
    {
        println!(
            "{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            t.format("%Y-%m-%d %H:%M:%S"),
            teme.position.x / 1000.0,
            teme.position.y / 1000.0,
            teme.position.z / 1000.0,
            teme.velocity.x / 1000.0,
            teme.velocity.y / 1000.0,
            teme.velocity.z / 1000.0,
        );
    }
}

/// Check if a transit is in progress at a given time, and determine that start and end bounds if
/// so.
#[test]
fn current_ground_station_pass() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(Degrees(55.8642), Degrees(-4.2518), 40.0);

    let start = p.epoch();
    let end = start + Duration::days(1);

    // Find the next transit so we have a known "mid-pass" time to hand to detect_transit.
    let transit: Transit = p
        .transits_iter(&gs, start..end, Degrees(5.0))
        .next()
        .expect("no transits during search interval")
        .expect("error calculating transit");

    // Simulate receiving the satellite 1/3 into the pass.
    let now = transit.start + (transit.end - transit.start) / 3;

    // Recover the full pass window from a single timestamp.
    let detected = p
        .detect_transit(now, &gs, Degrees(5.0))
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

/// Find the next ground station pass above 15° and sample the observations for it, including the
/// end time.
#[test]
fn next_ground_station_pass() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(Degrees(55.8642), Degrees(-4.2518), 40.0);

    let start = p.epoch();
    let end = start + Duration::days(1);

    // The iterator is lazy, so this scans only as far as the first transit.
    let next_transit = p
        .transits_iter(&gs, start..end, Degrees(15.0))
        .next()
        .expect("no transits during search interval")
        .expect("error calculating transit");

    println!("time,azimuth [deg], elevation [deg], range [km], range rate [km/s]");
    for observation in p
        // Transit implements IntervalRange, so it doubles as the interval here.
        .observation_iter(&gs, next_transit, Duration::seconds(10))
        // Sample the transit end time as well as the regular steps.
        .include_end()
    {
        let (t, observation) = observation.expect("error calculating observation");
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

/// Calculate ground station passes over 10° for the next 3 days
#[test]
fn ground_station_passes() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(Degrees(55.8642), Degrees(-4.2518), 40.0);

    let start = p.epoch();
    let end = start + Duration::days(3);

    struct GroundStationPass {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        aos: Observation, // Acquisition of signal
        los: Observation, // Loss of signal
        tca: Observation, // Time of closest approach
    }

    println!("start,end,aos_azimuth_deg,los_azimuth_deg,tca_elevation_deg,duration");
    for pass in p
        .transits_iter(&gs, start..end, Degrees(10.0))
        .skip_errors()
        .map(|t| GroundStationPass {
            start: t.start(),
            end: t.end(),
            aos: p
                .observe_at(t.start(), &gs)
                .expect("failed to calculate aos"),
            los: p.observe_at(t.end(), &gs).expect("failed to calculate los"),
            tca: p.max_elevation(t, &gs).expect("failed to calculate tca").1,
        })
    {
        println!(
            "{},{},{:.2},{:.2},{:.2},{}",
            pass.start.format("%Y-%m-%d %H:%M:%S"),
            pass.end.format("%Y-%m-%d %H:%M:%S"),
            pass.aos.azimuth.to_degrees(),
            pass.los.azimuth.to_degrees(),
            pass.tca.elevation.to_degrees(),
            humantime::format_duration(std::time::Duration::from_secs(
                (pass.end - pass.start).num_seconds() as u64
            ))
        )
    }
}

/// Calculate all sunlit windows for the satellite over the next 3 days.
#[test]
fn sunlight_windows() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();

    let start = p.epoch();
    let end = start + Duration::days(3);

    println!("start,end,duration");
    for window in p
        .illumination_iter(start..end)
        .skip_errors()
        .filter(|w| w.state == IlluminationState::Sunlit)
    {
        println!(
            "{},{},{}",
            window.start.format("%Y-%m-%d %H:%M:%S"),
            window.end.format("%Y-%m-%d %H:%M:%S"),
            humantime::format_duration(std::time::Duration::from_secs(
                window.duration().num_seconds() as u64
            )),
        );
    }
}

/// Find all transits above 30° over the next 3 days and clamp them to the eclipse sections only.
#[test]
fn eclipse_transits() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(Degrees(55.8642), Degrees(-4.2518), 40.0);

    let start = p.epoch();
    let end = start + Duration::days(3);

    // For each transit, compute illumination windows scoped to that pass and
    // retain only the eclipse portions.
    let mut n_transits = 0;
    let mut n_windows = 0;
    println!("start,end,duration");
    for transit in p
        .transits_iter(&gs, start..end, Degrees(30.0))
        .skip_errors()
    {
        n_transits += 1;
        for window in p
            .illumination_iter(transit)
            .skip_errors()
            .filter(|w| matches!(w.state, IlluminationState::Eclipse))
        {
            n_windows += 1;
            println!(
                "{},{},{:.0}",
                window.start.format("%Y-%m-%d %H:%M:%S"),
                window.end.format("%Y-%m-%d %H:%M:%S"),
                humantime::format_duration(std::time::Duration::from_secs(
                    window.duration().num_seconds() as u64
                )),
            );
        }
    }
    println!("{} transits filtered out", n_transits - n_windows);
}

/// Handle both iterator error classes in one scan: skip the individual passes that fail to refine,
/// but stop and report if propagation itself starts failing.
#[test]
fn resilient_pass_scan() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();
    let gs = GroundObserver::new(Degrees(55.8642), Degrees(-4.2518), 40.0);

    let start = p.epoch();
    let end = start + Duration::days(3);

    // A pass that fails to refine is a local failure — the scan has already moved past it, so the
    // next pass is unaffected. Drop it and keep a count. `skip_errors()` discards silently and
    // `log_errors()` logs at warn; `on_error` is the one that can also count.
    let mut unrefined = 0usize;
    let passes = p
        .transits_iter(&gs, start..end, Degrees(10.0))
        .on_error(|e| {
            unrefined += 1;
            tracing::warn!(error = %e, "skipping unrefined pass");
        });

    println!("start,end,samples");
    for transit in passes {
        // A propagation failure is deterministic in the elements, so it repeats at every sample: a
        // run of them means the TLE is unusable, not that one sample was awkward. Stop after three
        // in a row. `until_error()` is the zero-tolerance case.
        let mut samples = p
            .observation_iter(&gs, transit, Duration::seconds(10))
            .tolerate_errors(3);

        // Iterate `&mut` so the adapter survives the loop and can be asked why it stopped.
        let n = samples.by_ref().count();

        if let Some(e) = samples.error() {
            println!("propagation gave up mid-pass: {e}");
            break;
        }

        println!(
            "{},{},{n}",
            transit.start.format("%Y-%m-%d %H:%M:%S"),
            transit.end.format("%Y-%m-%d %H:%M:%S"),
        );
    }

    println!("{unrefined} passes dropped");
}

/// Calculate all apogee and perigee events for the next 3 days.
#[test]
fn apsides() {
    use sgp4_predict::ApsisEvent;

    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();

    let start = p.epoch();
    let end = start + Duration::days(3);

    println!("time,event,altitude [km]");
    for apsis in p.apsis_iter(start..end).skip_errors() {
        println!(
            "{},{},{:.3}",
            apsis.time.format("%Y-%m-%d %H:%M:%S"),
            match apsis.event {
                ApsisEvent::Apogee => "apogee",
                ApsisEvent::Perigee => "perigee",
            },
            apsis.altitude / 1000.0,
        );
    }
}
