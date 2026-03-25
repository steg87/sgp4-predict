use clap::{Parser, Subcommand};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(name = "sgp4-predict", about = "SGP4 satellite prediction CLI")]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Compute satellite observations over a time interval
    Observations(ObservationsArgs),
    /// Find satellite transits visible from an observer
    Transits(TransitsArgs),
}

/// Arguments shared by all subcommands.
#[derive(clap::Args)]
pub struct CommonArgs {
    /// Start time, e.g. "2026-03-25 10:00:00" or "2026-03-25T10:00:00Z" (default: now)
    #[arg(long)]
    pub start: Option<String>,

    /// Duration, e.g. "3d", "1h30m", "90s" (default: 1d)
    #[arg(long, value_parser = parse_duration, default_value = "1d")]
    pub duration: Duration,

    /// Observer as "lat_deg,lon_deg,alt_m" (e.g. "51.5,-0.1,0")
    #[arg(long)]
    pub observer: Option<String>,

    /// Path to TLE file (optional name line + line1 + line2)
    #[arg(long)]
    pub tle_file: Option<PathBuf>,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    pub out: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ObservationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Observation step (default: 60s)
    #[arg(long, value_parser = parse_duration, default_value = "60s")]
    pub step: Duration,
}

#[derive(clap::Args)]
pub struct TransitsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Minimum elevation above horizon in degrees (default: 10)
    #[arg(long, default_value = "10")]
    pub min_elevation: f64,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}
