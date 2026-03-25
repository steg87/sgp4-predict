use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::{Observation, Result as PredictResult, Transit};
use std::io::Write;

pub fn write_state_vectors<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, f64, f64, f64, f64, f64, f64)>>,
{
    // row width: 24 + (1+14)*3 + (1+12)*3 = 24+45+39 = 108
    writeln!(
        w,
        "{:<24} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "datetime", "x [km]", "y [km]", "z [km]", "vx [km/s]", "vy [km/s]", "vz [km/s]"
    )?;
    writeln!(w, "{}", "-".repeat(100))?;

    for item in iter {
        let (t, x, y, z, vx, vy, vz) = item.context("propagation error")?;
        writeln!(
            w,
            "{:<24} {:>12.3} {:>12.3} {:>12.3} {:>12.6} {:>12.6} {:>12.6}",
            t.format("%Y-%m-%dT%H:%M:%SZ"),
            x / 1_000.0,
            y / 1_000.0,
            z / 1_000.0,
            vx / 1_000.0,
            vy / 1_000.0,
            vz / 1_000.0,
        )?;
    }

    Ok(())
}

pub fn write_observations<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, Observation)>>,
{
    // row width: 24 + (1+8)*2 + (1+10) + (1+16) = 24+18+11+17 = 70
    writeln!(
        w,
        "{:<24} {:>8} {:>8} {:>10} {:>16}",
        "datetime", "az [deg]", "el [deg]", "range [km]", "range_rate [km/s]"
    )?;
    writeln!(w, "{}", "-".repeat(70))?;

    for item in iter {
        let (t, obs) = item.context("propagation error")?;
        writeln!(
            w,
            "{:<24} {:>8.2} {:>8.2} {:>10.2} {:>16.2}",
            t.format("%Y-%m-%dT%H:%M:%SZ"),
            obs.azimuth_deg(),
            obs.elevation_deg(),
            obs.range / 1_000.0,
            obs.range_rate / 1_000.0,
        )?;
    }

    Ok(())
}

pub fn write_transits<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = anyhow::Result<(Transit, Observation, Observation, Observation)>>,
{
    // row width: 24 + (1+24) + (1+12)*3 + (1+10) = 24+25+39+11 = 99
    writeln!(
        w,
        "{:<24} {:<24} {:>12} {:>12} {:>12} {:>10}",
        "aos", "los", "aos_az [deg]", "los_az [deg]", "tca_el [deg]", "duration"
    )?;
    writeln!(w, "{}", "-".repeat(99))?;

    for item in iter {
        let (transit, aos_obs, los_obs, tca_obs) = item?;
        let duration_secs = (transit.end - transit.start).num_seconds().max(0) as u64;
        let duration_str =
            humantime::format_duration(std::time::Duration::from_secs(duration_secs)).to_string();
        writeln!(
            w,
            "{:<24} {:<24} {:>12.2} {:>12.2} {:>12.2} {:>10}",
            transit.start.format("%Y-%m-%dT%H:%M:%SZ"),
            transit.end.format("%Y-%m-%dT%H:%M:%SZ"),
            aos_obs.azimuth_deg(),
            los_obs.azimuth_deg(),
            tca_obs.elevation_deg(),
            duration_str,
        )?;
    }

    Ok(())
}
