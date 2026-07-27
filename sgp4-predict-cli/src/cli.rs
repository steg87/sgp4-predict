use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    name = "sgp4-predict",
    about = "SGP4 satellite prediction CLI",
    version
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Path to the config file (default: ~/.sgp4-predict/config.yaml)
    #[arg(long, global = true, value_name = "PATH", long_help = CONFIG_LONG_HELP)]
    pub config: Option<PathBuf>,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress warnings on stderr
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
}

impl Args {
    /// Log filter implied by `--verbose` / `--quiet`, unless `RUST_LOG` overrides it.
    pub fn log_level(&self) -> &'static str {
        if self.quiet {
            return "error";
        }
        match self.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Compute satellite observations over a time interval
    Observations(ObservationsArgs),
    /// Find satellite transits visible from an observer
    Transits(TransitsArgs),
    /// Propagate state vectors over a time interval
    StateVectors(StateVectorsArgs),
    /// Find apogee and perigee events over a time interval
    Apsides(ApsidesArgs),
    /// Find illumination windows (sunlit/eclipse) over a time interval
    Illumination(IlluminationArgs),
}

/// Arguments shared by all subcommands.
#[derive(clap::Args)]
pub struct CommonArgs {
    /// Start time, e.g. "2026-03-25 10:00:00" or "2026-03-25T10:00:00Z" (default: now, always UTC)
    #[arg(long, value_parser = parse_start_time)]
    pub start: Option<DateTime<Utc>>,

    /// Duration, e.g. "3d", "1h30m", "90s"
    #[arg(long, value_parser = parse_duration, default_value = "1d")]
    pub duration: Duration,

    /// Path to TLE file (optional name line + line1 + line2); read from stdin if omitted
    #[arg(long, value_name = "PATH", long_help = TLE_FILE_LONG_HELP)]
    pub tle_file: Option<PathBuf>,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Prepend the resolved input arguments as # comment lines to the output
    #[arg(long)]
    pub output_args: bool,
}

const TLE_FILE_LONG_HELP: &str = "\
Path to a TLE file: an optional name line followed by line 1 and line 2.

If omitted, the TLE is read from stdin, so it can be piped in:

    cat sentinel.tle | sgp4-predict transits --gs glasgow";

const CONFIG_LONG_HELP: &str = "\
Path to the config file.

Defaults to .sgp4-predict/config.yaml under your home directory
(~/.sgp4-predict/config.yaml on Linux and macOS,
%USERPROFILE%\\.sgp4-predict\\config.yaml on Windows). A missing file at the
default path is not an error; a missing --config path is.

    groundstations:
      glasgow:
        location:
          latitude: 55.86
          longitude: -4.25
          altitude: 40";

/// Observer location arguments, shared by observations and transits.
///
/// `ObserverArgs::validate` enforces that `--gs` is present and names a station
/// the config defines; `ObserverArgs::resolve` turns it into a ground location.
#[derive(clap::Args)]
pub struct ObserverArgs {
    /// Ground station id from the config file
    #[arg(long, value_name = "ID", long_help = GS_LONG_HELP)]
    pub gs: Option<String>,
}

const GS_LONG_HELP: &str = "\
Ground station id, looked up in the `groundstations` map of the config file
(see --config).

Required. If the id is missing or unknown, the error lists the ids the config
does define.";

#[derive(clap::Args)]
pub struct ObservationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub observer: ObserverArgs,

    /// Observation step, e.g. "30s", "5m"
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,
}

#[derive(clap::Args)]
pub struct TransitsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub observer: ObserverArgs,

    /// Minimum elevation above the horizon, in degrees [-90, 90]
    #[arg(
        long = "min-elevation",
        value_name = "DEG",
        value_parser = parse_elevation,
        default_value = "0",
        allow_negative_numbers = true
    )]
    pub min_elevation_deg: f64,
}

#[derive(clap::Args)]
pub struct StateVectorsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Propagation step, e.g. "30s", "5m"
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,

    /// Coordinate frame for output
    #[arg(long, value_enum, default_value_t = Frame::Teme)]
    pub frame: Frame,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Frame {
    Teme,
    Ecef,
}

#[derive(clap::Args)]
pub struct ApsidesArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(clap::Args)]
pub struct IlluminationArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

fn parse_start_time(s: &str) -> Result<DateTime<Utc>, String> {
    humantime::parse_rfc3339_weak(s)
        .map(DateTime::<Utc>::from)
        .map_err(|e| e.to_string())
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

/// A step of zero never advances the scan, so reject it rather than hang.
fn parse_step(s: &str) -> Result<Duration, String> {
    let step = parse_duration(s)?;
    if step.is_zero() {
        return Err("step must be greater than zero".to_string());
    }
    Ok(step)
}

/// Elevation angles outside the horizon-to-zenith range can never be crossed.
fn parse_elevation(s: &str) -> Result<f64, String> {
    let deg: f64 = s.parse().map_err(|_| format!("invalid number: {s}"))?;
    if !(-90.0..=90.0).contains(&deg) {
        return Err(format!("elevation must be in [-90, 90] degrees, got {deg}"));
    }
    Ok(deg)
}
