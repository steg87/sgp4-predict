use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, time::Duration};

use crate::config::{AreaDef, BoxDef, CircleDef, EllipseDef, PolygonDef, Vertex};

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

#[derive(clap::Args)]
pub struct GsArgs {
    #[command(subcommand)]
    pub command: GsCommand,
}

#[derive(Subcommand)]
pub enum GsCommand {
    /// Add a ground station, prompting for each field
    Add,
    /// Remove a ground station
    #[command(alias = "rm")]
    Remove(GsRemoveArgs),
    /// List the ground stations in the config file
    #[command(alias = "ls")]
    List(GsListArgs),
}

#[derive(clap::Args)]
pub struct GsRemoveArgs {
    /// Ground station id to remove
    #[arg(value_name = "ID")]
    pub id: String,

    /// Remove without asking for confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(clap::Args)]
pub struct GsListArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,
}

#[derive(clap::Args)]
pub struct AoiCommandArgs {
    #[command(subcommand)]
    pub command: AoiCommand,
}

#[derive(Subcommand)]
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

#[derive(clap::Args)]
pub struct AoiAddArgs {
    /// Area id, used later as `--area <ID>`
    #[arg(value_name = "ID")]
    pub id: String,

    #[command(flatten)]
    pub shape: ShapeArgs,

    /// Replace an existing area with this id
    #[arg(short, long)]
    pub force: bool,
}

/// The four shapes an area may take. Exactly one is required.
///
/// Each parses straight into the stored [`AreaDef`], so a malformed shape is
/// reported by clap alongside the flag that produced it.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
pub struct ShapeArgs {
    /// Latitude/longitude box: centre, then longitude and latitude extents
    #[arg(
        long = "box",
        value_name = "LAT,LON,W,H",
        value_parser = parse_box,
        allow_hyphen_values = true,
        long_help = BOX_LONG_HELP
    )]
    pub r#box: Option<AreaDef>,

    /// Ellipse: centre, semi-major and semi-minor axes, optional bearing
    #[arg(
        long,
        value_name = "LAT,LON,A,B[,BEARING]",
        value_parser = parse_ellipse,
        allow_hyphen_values = true,
        long_help = ELLIPSE_LONG_HELP
    )]
    pub ellipse: Option<AreaDef>,

    /// Circle: centre and radius, in degrees of arc
    #[arg(
        long,
        value_name = "LAT,LON,R",
        value_parser = parse_circle,
        allow_hyphen_values = true,
        long_help = CIRCLE_LONG_HELP
    )]
    pub circle: Option<AreaDef>,

    /// Polygon: three or more (latitude,longitude) vertices
    #[arg(
        long,
        value_name = "(LAT,LON),(LAT,LON),...",
        value_parser = parse_poly,
        allow_hyphen_values = true,
        long_help = POLY_LONG_HELP
    )]
    pub poly: Option<AreaDef>,
}

impl ShapeArgs {
    /// The one shape that was given. The clap group guarantees exactly one.
    pub fn resolve(self) -> AreaDef {
        self.r#box
            .or(self.ellipse)
            .or(self.circle)
            .or(self.poly)
            .expect("the shape group is required")
    }
}

const BOX_LONG_HELP: &str = "\
Latitude/longitude box, as centre latitude, centre longitude, width, height.

Width is an extent in *longitude* and height an extent in latitude, both in
degrees, so the box's ground width shrinks with the cosine of its latitude.
The north and south edges follow their parallels exactly.

    --box 57,-4.5,7,6      # 54..60 N by 8 W..1 W";

const ELLIPSE_LONG_HELP: &str = "\
Ellipse, as centre latitude, centre longitude, semi-major axis, semi-minor
axis, and optionally the bearing of the major axis in degrees clockwise from
north (default 0, pointing at the pole).

Both axes are in degrees of arc — about 111.2 km per degree — and must satisfy
0 < semi-minor <= semi-major < 90.

    --ellipse 56,2,2.7,1.1,45      # roughly 300 x 120 km, pointing north-east";

const CIRCLE_LONG_HELP: &str = "\
Circle, as centre latitude, centre longitude, radius.

The radius is in degrees of arc — about 111.2 km per degree — and must be
under 90.

    --circle -33.9,18.4,2.25      # roughly 500 km across";

const POLY_LONG_HELP: &str = "\
Polygon, as three or more parenthesised (latitude,longitude) vertices.

The ring closes itself and vertex order does not matter. Edges are great-circle
arcs, so they are not lines of constant latitude — use --box when the region
really is a latitude/longitude box.

Parentheses are shell metacharacters, so quote the value:

    --poly \"(54,-8),(54,-1),(60,-1),(60,-8)\"";

#[derive(clap::Args)]
pub struct AoiRemoveArgs {
    /// Area id to remove
    #[arg(value_name = "ID")]
    pub id: String,

    /// Remove without asking for confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(clap::Args)]
pub struct AoiListArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,
}

#[derive(clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
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

/// Tabular output format.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
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

/// The CLI token that selects `value`, for `--output-args` headers.
/// Derived from the `ValueEnum` so it cannot drift from what clap accepts.
pub fn value_name(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .expect("no variant is skipped")
        .get_name()
        .to_string()
}

#[derive(clap::Args)]
pub struct ApsidesArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(clap::Args)]
pub struct GroundTrackArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Sampling step, e.g. "30s", "5m"
    #[arg(long, value_parser = parse_step, default_value = "60s")]
    pub step: Duration,
}

#[derive(clap::Args)]
pub struct AoiWindowsArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(flatten)]
    pub area: AreaArgs,
}

/// Area-of-interest selection, the counterpart of [`ObserverArgs`].
///
/// `AreaArgs::validate` enforces that `--area` is present and names an area
/// the config can build; `AreaArgs::resolve` returns the built shape.
#[derive(clap::Args)]
pub struct AreaArgs {
    /// Area of interest id from the config file
    #[arg(long, value_name = "ID", long_help = AREA_LONG_HELP)]
    pub area: Option<String>,
}

const AREA_LONG_HELP: &str = "\
Area of interest id, looked up in the `areas` map of the config file
(see --config).

Required. If the id is missing or unknown, the error lists the ids the config
does define. Add one with `sgp4-predict aoi add`.";

#[derive(clap::Args)]
pub struct IlluminationArgs {
    #[command(flatten)]
    pub common: CommonArgs,
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

/// Split a comma-separated list into numbers, naming the first bad field.
fn numbers(s: &str) -> Result<Vec<f64>, String> {
    s.split(',')
        .map(|field| {
            let field = field.trim();
            field
                .parse::<f64>()
                .map_err(|_| format!("expected a number, got '{field}'"))
        })
        .collect()
}

/// `LAT,LON,W,H` — centre, then the longitude and latitude extents.
///
/// Extents are checked here rather than by `Rectangle`, which sees only the
/// derived corners and so cannot say which input was wrong.
pub(crate) fn parse_box(s: &str) -> Result<AreaDef, String> {
    let values = numbers(s)?;
    let [latitude, longitude, width, height]: [f64; 4] = values[..]
        .try_into()
        .map_err(|_| format!("expected LAT,LON,W,H (4 values), got {}", values.len()))?;
    if !(width > 0.0 && width < 360.0) {
        return Err(format!("width must be in (0, 360) degrees, got {width}"));
    }
    if height <= 0.0 {
        return Err(format!(
            "height must be greater than 0 degrees, got {height}"
        ));
    }
    Ok(AreaDef::Box(BoxDef {
        latitude,
        longitude,
        width,
        height,
    }))
}

/// `LAT,LON,A,B` or `LAT,LON,A,B,BEARING`. The axes themselves are checked by
/// `Ellipse::new`, which owns the `0 < b <= a < 90` rule.
pub(crate) fn parse_ellipse(s: &str) -> Result<AreaDef, String> {
    let values = numbers(s)?;
    let (latitude, longitude, semi_major, semi_minor, bearing) = match values[..] {
        [lat, lon, a, b] => (lat, lon, a, b, 0.0),
        [lat, lon, a, b, bearing] => (lat, lon, a, b, bearing),
        _ => {
            return Err(format!(
                "expected LAT,LON,A,B or LAT,LON,A,B,BEARING (4 or 5 values), got {}",
                values.len()
            ));
        }
    };
    Ok(AreaDef::Ellipse(EllipseDef {
        latitude,
        longitude,
        semi_major,
        semi_minor,
        bearing,
    }))
}

/// `LAT,LON,R`.
pub(crate) fn parse_circle(s: &str) -> Result<AreaDef, String> {
    let values = numbers(s)?;
    let [latitude, longitude, radius]: [f64; 3] = values[..]
        .try_into()
        .map_err(|_| format!("expected LAT,LON,R (3 values), got {}", values.len()))?;
    Ok(AreaDef::Circle(CircleDef {
        latitude,
        longitude,
        radius,
    }))
}

/// `(LAT,LON),(LAT,LON),...`, with optional whitespace around the separators.
pub(crate) fn parse_poly(s: &str) -> Result<AreaDef, String> {
    let mut vertices = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let open = rest
            .strip_prefix('(')
            .ok_or_else(|| format!("expected '(' at \"{rest}\""))?;
        let (pair, tail) = open
            .split_once(')')
            .ok_or_else(|| format!("unclosed '(' at \"({open}\""))?;
        let values = numbers(pair)?;
        let [latitude, longitude]: [f64; 2] = values[..]
            .try_into()
            .map_err(|_| format!("each vertex is (LAT,LON), got '({pair})'"))?;
        vertices.push(Vertex {
            latitude,
            longitude,
        });
        rest = tail;
    }
    // Checked here as well as by `Polygon::new` so the message names the flag's
    // syntax rather than the library's deduplicated vertex count.
    if vertices.len() < 3 {
        return Err(format!(
            "a polygon needs at least 3 vertices, got {}",
            vertices.len()
        ));
    }
    Ok(AreaDef::Polygon(PolygonDef { vertices }))
}

/// Elevation angles outside the horizon-to-zenith range can never be crossed.
fn parse_elevation(s: &str) -> Result<f64, String> {
    let deg: f64 = s.parse().map_err(|_| format!("invalid number: {s}"))?;
    if !(-90.0..=90.0).contains(&deg) {
        return Err(format!("elevation must be in [-90, 90] degrees, got {deg}"));
    }
    Ok(deg)
}
