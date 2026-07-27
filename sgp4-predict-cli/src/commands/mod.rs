pub mod apsides;
pub mod gs;
pub mod illumination;
pub mod observations;
pub mod state_vectors;
pub mod transits;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use std::{
    io::{BufWriter, Write},
    ops::Range,
    path::Path,
};

use crate::{
    cli::{CommonArgs, Format},
    config, tle,
};
use sgp4_predict::{Observer, Predictor, Tle};

/// Everything the subcommands share: the resolved interval, the TLE and the
/// predictor built from it, and the writer to emit rows to.
///
/// Built by [`prepare`] so the five subcommands do not each repeat the
/// resolve-interval → load-TLE → build-predictor → open-writer sequence.
pub struct Context {
    pub start: DateTime<Utc>,
    pub interval: Range<DateTime<Utc>>,
    pub tle: Tle,
    pub predictor: Predictor,
    pub writer: Box<dyn Write>,
    pub format: Format,
}

impl Context {
    /// Write the `--output-args` header, if requested.
    ///
    /// `extra` carries the subcommand's own arguments; the shared ones are
    /// added here so every subcommand reports them identically.
    pub fn write_args_header(
        &mut self,
        command: &str,
        common: &CommonArgs,
        config_path: Option<&Path>,
        extra: &[(&str, &str)],
    ) -> anyhow::Result<()> {
        if !common.output_args {
            return Ok(());
        }

        let start = self.start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration = humantime::format_duration(common.duration).to_string();
        let format = crate::cli::value_name(self.format);
        let tle_source = common
            .tle_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "stdin".to_string());

        let mut pairs: Vec<(&str, &str)> = vec![
            ("command", command),
            ("satellite", &self.tle.satellite_name),
            ("tle-line1", &self.tle.line_1),
            ("tle-line2", &self.tle.line_2),
            ("tle-source", &tle_source),
            ("start", &start),
            ("duration", &duration),
        ];

        let config_display = config_path.map(|p| p.display().to_string());
        if let Some(path) = &config_display {
            pairs.push(("config", path));
        }
        pairs.extend_from_slice(extra);
        pairs.push(("format", &format));

        let out_display = common.out.as_ref().map(|p| p.display().to_string());
        if let Some(path) = &out_display {
            pairs.push(("out", path));
        }

        for (key, value) in &pairs {
            writeln!(self.writer, "# {key}: {value}")?;
        }
        Ok(())
    }
}

/// Resolve the shared inputs for a subcommand.
///
/// Ordering matters: the TLE is loaded before the writer is opened so a bad
/// TLE does not leave an empty `--out` file behind. Callers that take an
/// observer resolve it before calling this, so an unknown `--gs` fails before
/// the user is asked for a TLE on stdin.
pub fn prepare(common: &CommonArgs) -> anyhow::Result<Context> {
    // `#` comment lines are not valid JSON and break strict CSV readers.
    // Checked before anything is opened so no partial --out file is left.
    anyhow::ensure!(
        !common.output_args || common.format == Format::Text,
        "--output-args is only supported with --format text"
    );

    let start = common.start.unwrap_or_else(Utc::now);
    let dur = chrono::Duration::from_std(common.duration).context("duration out of range")?;
    let interval = start..start + dur;

    let tle = load_tle(common.tle_file.as_deref())?;
    let predictor = Predictor::from_tle(&tle)?;
    warn_stale_tle(&predictor, start);
    let writer = open_writer(common.out.as_deref())?;

    Ok(Context {
        start,
        interval,
        tle,
        predictor,
        writer,
        format: common.format,
    })
}

/// Load a TLE from `--tle-file`, or from stdin when the flag is omitted.
pub fn load_tle(tle_file: Option<&Path>) -> anyhow::Result<Tle> {
    match tle_file {
        Some(p) => tle::parse_tle_file(p),
        None => tle::read_tle_stdin(),
    }
}

/// Log the TLE age and warn to stderr if it exceeds 7 days.
pub fn warn_stale_tle(predictor: &Predictor, start: DateTime<Utc>) {
    let age = predictor.tle_age(start);
    let age_days = age.num_days();
    tracing::info!(tle_age_days = age_days, "predictor ready");
    if age.num_hours() > 7 * 24 {
        tracing::warn!("TLE is {age_days} days old; SGP4 accuracy may be degraded");
    }
}

/// Open a buffered writer to a file, or to stdout if no path is given.
pub fn open_writer(out: Option<&Path>) -> anyhow::Result<Box<dyn Write>> {
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

/// Resolve the config path actually in use, for `--output-args`.
pub fn effective_config_path(explicit: Option<&Path>) -> Option<std::path::PathBuf> {
    explicit
        .map(Path::to_path_buf)
        .or_else(config::default_path)
}
