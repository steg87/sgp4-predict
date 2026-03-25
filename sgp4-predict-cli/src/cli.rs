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
}

#[derive(clap::Args)]
pub struct ObservationsArgs {
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

    /// Observation step (default: 60s)
    #[arg(long, value_parser = parse_duration, default_value = "60s")]
    pub step: Duration,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    pub out: Option<PathBuf>,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}
