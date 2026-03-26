use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::{
    Apsis, ApsisEvent, Illumination, IlluminationState, Observation, Result as PredictResult,
    Transit,
};
use std::io::Write;

pub fn write_apsides<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<Apsis>>,
{
    // row width: 24 + (1+10) + (1+14) = 50
    writeln!(w, "{:<24} {:>10} {:>14}", "time", "event", "altitude [km]")?;
    writeln!(w, "{}", "-".repeat(50))?;

    for item in iter {
        let apsis = item.context("apsis detection error")?;
        let event = match apsis.event {
            ApsisEvent::Apogee => "Apogee",
            ApsisEvent::Perigee => "Perigee",
        };
        writeln!(
            w,
            "{:<24} {:>10} {:>14.3}",
            apsis.time.format("%Y-%m-%dT%H:%M:%SZ"),
            event,
            apsis.altitude / 1_000.0,
        )?;
    }

    Ok(())
}

pub fn write_illumination<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<Illumination>>,
{
    // row width: 24 + (1+24) + (1+10) + (1+10) = 71
    writeln!(
        w,
        "{:<24} {:<24} {:>10} {:>10}",
        "start", "end", "state", "duration"
    )?;
    writeln!(w, "{}", "-".repeat(71))?;

    for item in iter {
        let window = item.context("illumination detection error")?;
        let state = match window.state {
            IlluminationState::Sunlit => "Sunlit",
            IlluminationState::Eclipse => "Eclipse",
        };
        let duration_secs = (window.end - window.start).num_seconds().max(0) as u64;
        let duration_str =
            humantime::format_duration(std::time::Duration::from_secs(duration_secs)).to_string();
        writeln!(
            w,
            "{:<24} {:<24} {:>10} {:>10}",
            window.start.format("%Y-%m-%dT%H:%M:%SZ"),
            window.end.format("%Y-%m-%dT%H:%M:%SZ"),
            state,
            duration_str,
        )?;
    }

    Ok(())
}

pub fn write_state_vectors<W, I>(mut w: W, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, f64, f64, f64, f64, f64, f64)>>,
{
    // row width: 24 + (1+12)*6 = 24+78 = 102
    writeln!(
        w,
        "{:<24} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "datetime", "x [km]", "y [km]", "z [km]", "vx [km/s]", "vy [km/s]", "vz [km/s]"
    )?;
    writeln!(w, "{}", "-".repeat(102))?;

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
    I: Iterator<
        Item = anyhow::Result<(
            Transit,
            Observation,
            Observation,
            DateTime<Utc>,
            Observation,
        )>,
    >,
{
    // row width: 24 + (1+24) + (1+12)*2 + (1+24) + (1+12) + (1+10) = 24+25+26+25+13+11 = 124
    writeln!(
        w,
        "{:<24} {:<24} {:>12} {:>12} {:<24} {:>12} {:>10}",
        "aos", "los", "aos_az [deg]", "los_az [deg]", "tca_time", "tca_el [deg]", "duration"
    )?;
    writeln!(w, "{}", "-".repeat(124))?;

    for item in iter {
        let (transit, aos_obs, los_obs, tca_time, tca_obs) = item?;
        let duration_secs = (transit.end - transit.start).num_seconds().max(0) as u64;
        let duration_str =
            humantime::format_duration(std::time::Duration::from_secs(duration_secs)).to_string();
        writeln!(
            w,
            "{:<24} {:<24} {:>12.2} {:>12.2} {:<24} {:>12.2} {:>10}",
            transit.start.format("%Y-%m-%dT%H:%M:%SZ"),
            transit.end.format("%Y-%m-%dT%H:%M:%SZ"),
            aos_obs.azimuth_deg(),
            los_obs.azimuth_deg(),
            tca_time.format("%Y-%m-%dT%H:%M:%SZ"),
            tca_obs.elevation_deg(),
            duration_str,
        )?;
    }

    Ok(())
}
