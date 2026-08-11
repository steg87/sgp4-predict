use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use sgp4_predict::Coverage;
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "sgp4-predict",
    about = "SGP4 satellite prediction CLI",
    version
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Path to the config file (default: ~/.sgp4-predict/config.yaml)
    #[arg(long, global = true, value_name = "PATH", value_parser = parse_path, long_help = CONFIG_LONG_HELP)]
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

#[derive(Debug, Clone, Subcommand)]
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
    /// Trace the sub-satellite point over a time interval
    GroundTrack(GroundTrackArgs),
    /// Find the windows where the ground track is inside an area of interest
    AoiWindows(AoiWindowsArgs),
    /// Manage the ground stations in the config file
    Gs(GsArgs),
    /// Manage the areas of interest in the config file
    Aoi(AoiCommandArgs),
    /// Generate a shell completion script on stdout
    Completions(CompletionsArgs),
    /// Generate a roff man page on stdout
    Man,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GsArgs {
    #[command(subcommand)]
    pub command: GsCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum GsCommand {
    /// Add a ground station, prompting for each field
    Add(GsAddArgs),
    /// Remove a ground station
    #[command(alias = "rm")]
    Remove(GsRemoveArgs),
    /// List the ground stations in the config file
    #[command(alias = "ls")]
    List(GsListArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct GsAddArgs {
    /// Ground station id, used later as `--gs <ID>`; prompted for if omitted
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Replace an existing station with this id
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GsRemoveArgs {
    /// Ground station id to remove
    #[arg(value_name = "ID")]
    pub id: String,

    /// Remove without asking for confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GsListArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AoiCommandArgs {
    #[command(subcommand)]
    pub command: AoiCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AoiCommand {
    /// Add an area of interest
    Add(AoiAddArgs),
    /// Remove an area of interest
    #[command(alias = "rm")]
    Remove(AoiRemoveArgs),
    /// List the areas of interest in the config file
    #[command(alias = "ls")]
    List(AoiListArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct AoiAddArgs {
    /// AOI id, used later as `--aoi <ID>`; prompted for if omitted
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Which shape the AOI takes; prompted for if omitted
    #[arg(long, value_enum, long_help = SHAPE_LONG_HELP)]
    pub shape: Option<Shape>,

    /// Replace an existing AOI with this id
    #[arg(short, long)]
    pub force: bool,
}

/// The shape an AOI takes. Its *definition* is always prompted for — there is
/// deliberately no flag carrying coordinates, matching `gs add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum Shape {
    /// Latitude/longitude box, given by its south/north/west/east bounds
    Box,
    /// Circle, given by its centre and radius
    Circle,
    /// Ring of three or more vertices
    Polygon,
}

const SHAPE_LONG_HELP: &str = "\
Which shape the AOI takes.

Only the shape is taken here; its coordinates are always prompted for, as they
are for `gs add`. Edit the config file directly to write an AOI out by hand.

    sgp4-predict aoi add scotland --shape box";

#[derive(Debug, Clone, clap::Args)]
pub struct AoiRemoveArgs {
    /// AOI id to remove
    #[arg(value_name = "ID")]
    pub id: String,

    /// Remove without asking for confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AoiListArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,
}

#[derive(Debug, Clone, clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

/// Arguments shared by all subcommands.
#[derive(Debug, Clone, clap::Args)]
pub struct CommonArgs {
    /// Start time, e.g. "2026-03-25 10:00:00" or "2026-03-25T10:00:00Z" (default: now, always UTC)
    #[arg(long, value_parser = parse_start_time)]
    pub start: Option<DateTime<Utc>>,

    /// Duration, e.g. "3d", "1h30m", "90s"
    #[arg(long, value_parser = parse_duration, default_value = "1d")]
    pub duration: Duration,

    /// Path to TLE file (optional name line + line1 + line2); read from stdin if omitted
    #[arg(long, value_name = "PATH", value_parser = parse_path, long_help = TLE_FILE_LONG_HELP)]
    pub tle_file: Option<PathBuf>,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long, value_name = "PATH", value_parser = parse_path)]
    pub out: Option<PathBuf>,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Prepend the resolved input arguments as # comment lines to the output
    #[arg(long)]
    pub output_args: bool,
}

/// How much of an AOI must be within reach for a window to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum CoverageArg {
    /// Any part of the area is within reach
    Any,
    /// Every part of the area is within reach at once
    Full,
}

impl From<CoverageArg> for Coverage {
    fn from(c: CoverageArg) -> Self {
        match c {
            CoverageArg::Any => Self::Any,
            CoverageArg::Full => Self::Full,
        }
    }
}

/// Tabular output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum Format {
    /// Fixed-width columns with a header, for reading
    Text,
    /// One JSON object per row, newline-delimited
    Json,
    /// RFC 4180 comma-separated values with a header row
    Csv,
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
#[derive(Debug, Clone, clap::Args)]
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

#[derive(Debug, Clone, clap::Args)]
pub struct ObservationsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub observer: ObserverArgs,

    /// Observation step, e.g. "30s", "5m"
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,
}

#[derive(Debug, Clone, clap::Args)]
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

    #[command(flatten)]
    pub tuning: TransitTuningArgs,

    #[command(flatten)]
    pub refinement: RefinementArgs,
}

/// `TransitIterOpts` and `MaxElevationOpts` as flags. Defaults mirror those
/// structs' `Default` impls, so passing none of these reproduces `transits_iter`.
#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Detection tuning")]
pub struct TransitTuningArgs {
    /// Lower bound of the adaptive coarse-scan step
    #[arg(long, value_parser = parse_step, default_value = "10s")]
    pub min_step: Duration,

    /// Upper bound of the adaptive coarse-scan step
    #[arg(long, value_parser = parse_step, default_value = "10m")]
    pub max_step: Duration,

    /// Fixed step used to walk from a transit's start to its end
    #[arg(long, value_parser = parse_step, default_value = "30s")]
    pub walk_step: Duration,

    /// A transit longer than this is reported as an error
    #[arg(long, value_parser = parse_positive_duration, default_value = "1h")]
    pub max_transit_duration: Duration,

    /// Discard a transit already in progress at the interval start; false walks
    /// backward past it to find its true AoS
    #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set, default_value_t = true)]
    pub skip_leading_partial: bool,

    /// Clamp a transit still in progress at the interval end to the interval;
    /// false walks forward past it to find its true LoS
    #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set, default_value_t = false)]
    pub clamp_to_interval: bool,

    /// Fixed step used to scan for the time of closest approach
    #[arg(long, value_parser = parse_step, default_value = "10s")]
    pub tca_scan_step: Duration,
}

#[derive(Debug, Clone, clap::Args)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum Frame {
    Teme,
    Ecef,
}

/// The CLI token that selects `value`, for `--output-args` headers.
/// Derived from the `ValueEnum` so it cannot drift from what clap accepts.
pub fn value_name(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .expect("no variant is skipped")
        .get_name()
        .to_string()
}

#[derive(Debug, Clone, clap::Args)]
pub struct ApsidesArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub tuning: ApsisTuningArgs,

    #[command(flatten)]
    pub refinement: RefinementArgs,
}

/// `ApsisIterOpts` as flags.
#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Detection tuning")]
pub struct ApsisTuningArgs {
    /// Fixed step used to scan for radial-velocity sign changes
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,
}

#[derive(Debug, Clone, clap::Args)]
pub struct GroundTrackArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Sampling step, e.g. "30s", "5m"
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AoiWindowsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub aoi: AoiArgs,

    #[command(flatten)]
    pub tuning: AoiTuningArgs,

    #[command(flatten)]
    pub refinement: RefinementArgs,
}

/// `AoiIterOpts` as flags.
#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Detection tuning")]
pub struct AoiTuningArgs {
    /// Half-angle of the satellite's field of regard, in degrees — the largest
    /// nadir angle the payload can be slewed to. Zero detects the ground track
    /// itself crossing the area
    #[arg(long, value_name = "DEG", default_value_t = 0.0)]
    pub max_off_nadir: f64,

    /// Whether any part of the area or all of it must be within reach
    #[arg(long, value_enum, default_value_t = CoverageArg::Any)]
    pub coverage: CoverageArg,

    /// Lower bound of the adaptive coarse-scan step, and so the shortest
    /// crossing the scan is guaranteed to see. Floored at 1ms
    #[arg(long, value_parser = parse_step, default_value = "1s")]
    pub min_step: Duration,

    /// Upper bound of the adaptive coarse-scan step, used far from the area
    #[arg(long, value_parser = parse_step, default_value = "10m")]
    pub max_step: Duration,

    /// Fixed step used to walk from a window's start to its end
    #[arg(long, value_parser = parse_step, default_value = "5s")]
    pub walk_step: Duration,

    /// A window longer than this is reported as an error; raise it for a
    /// continental-scale area
    #[arg(long, value_parser = parse_positive_duration, default_value = "1h")]
    pub max_window_duration: Duration,

    /// Discard a window already in progress at the interval start; false walks
    /// backward past it to find its true beginning
    #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set, default_value_t = true)]
    pub skip_leading_partial: bool,

    /// Clamp a window still in progress at the interval end to the interval;
    /// false walks forward past it to find its true end
    #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set, default_value_t = false)]
    pub clamp_to_interval: bool,
}

/// Area-of-interest selection, the counterpart of [`ObserverArgs`].
///
/// `AoiArgs::validate` enforces that `--aoi` is present and names an AOI the
/// config can build; `AoiArgs::resolve` returns the built shape.
#[derive(Debug, Clone, clap::Args)]
pub struct AoiArgs {
    /// Area of interest id from the config file
    #[arg(long = "aoi", value_name = "ID", long_help = AOI_LONG_HELP)]
    pub id: Option<String>,
}

const AOI_LONG_HELP: &str = "\
Area of interest id, looked up in the `aois` map of the config file
(see --config).

Required. If the id is missing or unknown, the error lists the ids the config
does define. Add one with `sgp4-predict aoi add`.";

#[derive(Debug, Clone, clap::Args)]
pub struct IlluminationArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub tuning: IlluminationTuningArgs,

    #[command(flatten)]
    pub refinement: RefinementArgs,
}

/// `IlluminationIterOpts` as flags.
#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Detection tuning")]
pub struct IlluminationTuningArgs {
    /// Fixed step used to scan for shadow-boundary crossings
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,

    /// Fixed step used to walk out to a window's true start and end
    #[arg(long, value_parser = parse_step, default_value = "30s")]
    pub walk_step: Duration,

    /// An eclipse window longer than this is reported as an error; sunlit
    /// windows are the gaps between eclipses and are never capped
    #[arg(long, value_parser = parse_positive_duration, default_value = "1h")]
    pub max_window_duration: Duration,
}

/// Root-finder configuration, shared by every detection subcommand.
///
/// Applied with `Predictor::with_refinement` rather than passed per call, so it
/// reaches the one-shot refinements (`max_elevation`) as well as the iterators.
#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Root finder")]
pub struct RefinementArgs {
    /// Convergence threshold on the bracket width, in seconds
    #[arg(long, value_name = "SECONDS", value_parser = parse_time_tolerance, default_value = "0.001")]
    pub time_tolerance: f64,

    /// Maximum root-finder iterations before reporting failure to converge
    #[arg(long, value_name = "N", value_parser = parse_max_iter, default_value = "100")]
    pub max_iter: usize,
}

/// Expand a leading `~` or `~/`, so a quoted path behaves like an unquoted one.
/// `~user/` is not supported and is left alone.
fn parse_path(s: &str) -> Result<PathBuf, String> {
    let rest = match s {
        "~" => Some(""),
        _ => s.strip_prefix("~/"),
    };
    match rest {
        Some(rest) => match dirs::home_dir() {
            Some(home) => Ok(home.join(rest)),
            None => Err("cannot expand '~': no home directory".to_string()),
        },
        None => Ok(PathBuf::from(s)),
    }
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

/// A zero cap rejects every window, so no result could ever be reported.
fn parse_positive_duration(s: &str) -> Result<Duration, String> {
    let duration = parse_duration(s)?;
    if duration.is_zero() {
        return Err("duration must be greater than zero".to_string());
    }
    Ok(duration)
}

/// A non-positive tolerance can never be met, so iteration would always run to
/// `max_iter` and report a failure to converge.
fn parse_time_tolerance(s: &str) -> Result<f64, String> {
    let seconds: f64 = s.parse().map_err(|_| format!("invalid number: {s}"))?;
    if !(seconds.is_finite() && seconds > 0.0) {
        return Err(format!("time tolerance must be greater than zero, got {s}"));
    }
    Ok(seconds)
}

/// Zero iterations refines nothing, so every crossing would fail to converge.
fn parse_max_iter(s: &str) -> Result<usize, String> {
    match s.parse() {
        Ok(0) | Err(_) => Err(format!("max iterations must be at least 1, got {s}")),
        Ok(n) => Ok(n),
    }
}

/// Elevation angles outside the horizon-to-zenith range can never be crossed.
fn parse_elevation(s: &str) -> Result<f64, String> {
    let deg: f64 = s.parse().map_err(|_| format!("invalid number: {s}"))?;
    if !(-90.0..=90.0).contains(&deg) {
        return Err(format!("elevation must be in [-90, 90] degrees, got {deg}"));
    }
    Ok(deg)
}
