pub mod apsides;
pub mod illumination;
pub mod observations;
pub mod state_vectors;
pub mod transits;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::Predictor;
use std::{
    io::{BufWriter, Write},
    ops::Range,
    path::PathBuf,
};

use crate::{cli::CommonArgs, tle};
use sgp4_predict::{Observer, Tle};

/// Resolve the start time and interval from common args.
pub fn resolve_interval(
    common: &CommonArgs,
) -> anyhow::Result<(DateTime<Utc>, Range<DateTime<Utc>>)> {
    let start = common.start.unwrap_or_else(Utc::now);
    let dur = chrono::Duration::from_std(common.duration).context("duration out of range")?;
    Ok((start, start..start + dur))
}

/// Load a TLE from file or prompt interactively.
pub fn load_tle(common: &CommonArgs) -> anyhow::Result<Tle> {
    match &common.tle_file {
        Some(p) => tle::parse_tle_file(p),
        None => tle::prompt_tle(),
    }
}

/// Log the TLE age and warn to stderr if it exceeds 7 days.
pub fn warn_stale_tle(predictor: &Predictor, start: DateTime<Utc>) {
    let age = predictor.tle_age(start);
    let age_days = age.num_days();
    tracing::info!(tle_age_days = age_days, "predictor ready");
    if age.num_hours() > 7 * 24 {
        eprintln!("warning: TLE is {age_days} days old; SGP4 accuracy may be degraded");
    }
}

/// Open a buffered writer to a file, or to stdout if no path is given.
pub fn open_writer(out: &Option<PathBuf>) -> anyhow::Result<Box<dyn Write>> {
    match out {
        Some(path) => Ok(Box::new(BufWriter::new(
            std::fs::File::create(path)
                .with_context(|| format!("failed to create {}", path.display()))?,
        ))),
        None => Ok(Box::new(BufWriter::new(std::io::stdout()))),
    }
}

/// Format observer as a "lat,lon,alt" string for --output-args headers.
pub fn format_observer_str(obs: &impl Observer) -> String {
    format!(
        "{},{},{}",
        obs.latitude().to_f64(),
        obs.longitude().to_f64(),
        obs.altitude()
    )
}

/// Write CLI argument pairs as `# key: value` comment lines.
pub fn write_args_header(w: &mut dyn Write, pairs: &[(&str, &str)]) -> anyhow::Result<()> {
    for (key, value) in pairs {
        writeln!(w, "# {key}: {value}")?;
    }
    Ok(())
}
