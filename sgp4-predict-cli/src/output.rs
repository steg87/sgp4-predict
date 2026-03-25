use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::{Observation, Result as PredictResult, Transit};
use std::io::Write;

pub fn write_observations<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, Observation)>>,
{
    writeln!(
        w,
        "{:<24} {:>8} {:>8} {:>10} {:>14}",
        "datetime", "az_deg", "el_deg", "range_km", "range_rate_m_s"
    )?;
    writeln!(w, "{}", "-".repeat(68))?;

    for item in iter {
        let (t, obs) = item.context("propagation error")?;
        writeln!(
            w,
            "{:<24} {:>8.2} {:>8.2} {:>10.2} {:>14.2}",
            t.format("%Y-%m-%dT%H:%M:%SZ"),
            obs.azimuth_deg(),
            obs.elevation_deg(),
            obs.range / 1_000.0,
            obs.range_rate,
        )?;
    }

    Ok(())
}

pub fn write_transits<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = anyhow::Result<(Transit, Observation, Observation, Observation)>>,
{
    writeln!(
        w,
        "{:<24} {:<24} {:>10} {:>10} {:>12} {:>10}",
        "aos", "los", "aos_az_deg", "los_az_deg", "tca_el_deg", "duration"
    )?;
    writeln!(w, "{}", "-".repeat(94))?;

    for item in iter {
        let (transit, aos_obs, los_obs, tca_obs) = item?;
        let duration_secs = (transit.end - transit.start).num_seconds().max(0) as u64;
        let duration_str =
            humantime::format_duration(std::time::Duration::from_secs(duration_secs)).to_string();
        writeln!(
            w,
            "{:<24} {:<24} {:>10.2} {:>10.2} {:>12.2} {:>10}",
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
