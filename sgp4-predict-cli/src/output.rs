use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::{Observation, Result as PredictResult};
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
