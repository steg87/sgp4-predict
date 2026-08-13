//! Row-oriented output in the formats offered by `--format`.
//!
//! Each `write_*` function turns an iterator of library results into rows and
//! hands them to a [`RowWriter`], so adding a format means adding a variant
//! here rather than touching every command.

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::{
    AoiWindow, Apsis, ApsisEvent, Geodetic, Illumination, IlluminationState, Observation,
    Result as PredictResult, Transit,
};
use std::io::Write;

use crate::cli::Format;

/// A single output cell. Text and CSV render it as-is; JSON needs to know
/// whether to quote it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Cell {
    Str(String),
    Num(String),
    /// A value with no representation in the output. JSON emits `null`; text
    /// and CSV leave the field empty, which is the convention there.
    Null,
}

impl Cell {
    fn text(&self) -> &str {
        match self {
            Cell::Str(s) | Cell::Num(s) => s,
            Cell::Null => "",
        }
    }
}

fn time(t: DateTime<Utc>) -> Cell {
    Cell::Str(t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn num(value: f64, precision: usize) -> Cell {
    // NaN and infinity have no JSON representation; a bare `NaN` would make
    // the output unparseable.
    if value.is_finite() {
        Cell::Num(format!("{value:.precision$}"))
    } else {
        Cell::Null
    }
}

fn text(s: impl Into<String>) -> Cell {
    Cell::Str(s.into())
}

/// Seconds between two instants, formatted like "10m 48s". Never negative.
fn duration(start: DateTime<Utc>, end: DateTime<Utc>) -> Cell {
    let secs = (end - start).num_seconds().max(0) as u64;
    Cell::Str(humantime::format_duration(std::time::Duration::from_secs(secs)).to_string())
}

/// A column: its header, its width in text mode, and whether text mode
/// right-aligns it. JSON uses `key` for the field name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Column {
    header: &'static str,
    key: &'static str,
    width: usize,
    right: bool,
}

const fn left(header: &'static str, key: &'static str, width: usize) -> Column {
    Column {
        header,
        key,
        width,
        right: false,
    }
}

const fn right(header: &'static str, key: &'static str, width: usize) -> Column {
    Column {
        header,
        key,
        width,
        right: true,
    }
}

/// Writes rows in the selected format, emitting any header on the first row.
#[derive(Debug)]
struct RowWriter<'a, W: Write> {
    w: W,
    format: Format,
    columns: &'a [Column],
    header_written: bool,
}

impl<'a, W: Write> RowWriter<'a, W> {
    fn new(w: W, format: Format, columns: &'a [Column]) -> Self {
        Self {
            w,
            format,
            columns,
            header_written: false,
        }
    }

    fn write_header(&mut self) -> anyhow::Result<()> {
        match self.format {
            Format::Text => {
                let header = text_row(self.columns, self.columns.iter().map(|c| c.header));
                // Underline is derived from the rendered header, so a column
                // width change cannot desync it.
                writeln!(self.w, "{header}")?;
                writeln!(self.w, "{}", "-".repeat(header.chars().count()))?;
            }
            Format::Csv => {
                let header: Vec<&str> = self.columns.iter().map(|c| c.key).collect();
                writeln!(self.w, "{}", header.join(","))?;
            }
            Format::Json => {}
        }
        Ok(())
    }

    fn write_row(&mut self, cells: &[Cell]) -> anyhow::Result<()> {
        debug_assert_eq!(cells.len(), self.columns.len(), "row/column count mismatch");

        if !self.header_written {
            self.write_header()?;
            self.header_written = true;
        }

        match self.format {
            Format::Text => {
                writeln!(
                    self.w,
                    "{}",
                    text_row(self.columns, cells.iter().map(Cell::text))
                )?;
            }
            Format::Csv => {
                let fields: Vec<String> = cells.iter().map(|c| csv_field(c.text())).collect();
                writeln!(self.w, "{}", fields.join(","))?;
            }
            Format::Json => {
                let fields: Vec<String> = self
                    .columns
                    .iter()
                    .zip(cells)
                    .map(|(column, cell)| match cell {
                        Cell::Num(v) => format!("\"{}\":{}", column.key, v),
                        Cell::Null => format!("\"{}\":null", column.key),
                        Cell::Str(v) => format!("\"{}\":\"{}\"", column.key, json_escape(v)),
                    })
                    .collect();
                writeln!(self.w, "{{{}}}", fields.join(","))?;
            }
        }
        Ok(())
    }

    /// Emit the header even when no rows followed, so an empty result still
    /// identifies its columns. JSON stays empty — a header would not be valid.
    ///
    /// Flushes before returning: `BufWriter`'s `Drop` also flushes but discards
    /// the error, which would let a full disk or a closed pipe truncate the
    /// output while the command still exited 0.
    fn finish(&mut self) -> anyhow::Result<()> {
        if !self.header_written {
            self.write_header()?;
            self.header_written = true;
        }
        self.w.flush().context("failed to write output")?;
        Ok(())
    }
}

/// Pad `values` into fixed-width columns separated by a single space.
/// Trailing padding is trimmed so the header and its underline agree.
fn text_row<'v>(columns: &[Column], values: impl Iterator<Item = &'v str>) -> String {
    let mut out = String::new();
    for (i, (column, value)) in columns.iter().zip(values).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // A value wider than its column would push the rest of the row past the
        // header underline, so truncate with an ellipsis. Only ids are
        // user-supplied and long enough for this to bite.
        let value = if value.chars().count() > column.width {
            let keep: String = value.chars().take(column.width.saturating_sub(1)).collect();
            format!("{keep}…")
        } else {
            value.to_string()
        };
        if column.right {
            out.push_str(&format!("{value:>width$}", width = column.width));
        } else {
            out.push_str(&format!("{value:<width$}", width = column.width));
        }
    }
    out.trim_end().to_string()
}

/// Quote per RFC 4180 when the field contains a comma, quote, or newline.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

const APSIS_COLUMNS: &[Column] = &[
    left("time", "time", 24),
    right("event", "event", 10),
    right("altitude [km]", "altitude_km", 14),
];

pub fn write_apsides<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<Apsis>>,
{
    let mut out = RowWriter::new(w, format, APSIS_COLUMNS);
    for item in iter {
        let apsis = item.context("apsis detection error")?;
        let event = match apsis.event {
            ApsisEvent::Apogee => "Apogee",
            ApsisEvent::Perigee => "Perigee",
        };
        out.write_row(&[
            time(apsis.time),
            text(event),
            num(apsis.altitude / 1_000.0, 3),
        ])?;
    }
    out.finish()
}

const ILLUMINATION_COLUMNS: &[Column] = &[
    left("start", "start", 24),
    left("end", "end", 24),
    right("state", "state", 10),
    right("duration", "duration", 10),
];

pub fn write_illumination<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<Illumination>>,
{
    let mut out = RowWriter::new(w, format, ILLUMINATION_COLUMNS);
    for item in iter {
        let window = item.context("illumination detection error")?;
        let state = match window.state {
            IlluminationState::Sunlit => "Sunlit",
            IlluminationState::Eclipse => "Eclipse",
        };
        out.write_row(&[
            time(window.start),
            time(window.end),
            text(state),
            duration(window.start, window.end),
        ])?;
    }
    out.finish()
}

const STATE_VECTOR_COLUMNS: &[Column] = &[
    left("datetime", "datetime", 24),
    right("x [km]", "x_km", 12),
    right("y [km]", "y_km", 12),
    right("z [km]", "z_km", 12),
    right("vx [km/s]", "vx_km_s", 12),
    right("vy [km/s]", "vy_km_s", 12),
    right("vz [km/s]", "vz_km_s", 12),
];

pub fn write_state_vectors<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, f64, f64, f64, f64, f64, f64)>>,
{
    let mut out = RowWriter::new(w, format, STATE_VECTOR_COLUMNS);
    for item in iter {
        let (t, x, y, z, vx, vy, vz) = item.context("propagation error")?;
        out.write_row(&[
            time(t),
            num(x / 1_000.0, 3),
            num(y / 1_000.0, 3),
            num(z / 1_000.0, 3),
            num(vx / 1_000.0, 6),
            num(vy / 1_000.0, 6),
            num(vz / 1_000.0, 6),
        ])?;
    }
    out.finish()
}

const OBSERVATION_COLUMNS: &[Column] = &[
    left("datetime", "datetime", 24),
    right("az [deg]", "az_deg", 8),
    right("el [deg]", "el_deg", 8),
    right("range [km]", "range_km", 10),
    right("range_rate [km/s]", "range_rate_km_s", 16),
];

pub fn write_observations<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, Observation)>>,
{
    let mut out = RowWriter::new(w, format, OBSERVATION_COLUMNS);
    for item in iter {
        let (t, obs) = item.context("propagation error")?;
        out.write_row(&[
            time(t),
            num(obs.azimuth.to_degrees().to_f64(), 2),
            num(obs.elevation.to_degrees().to_f64(), 2),
            num(obs.range / 1_000.0, 2),
            num(obs.range_rate / 1_000.0, 2),
        ])?;
    }
    out.finish()
}

const TRANSIT_COLUMNS: &[Column] = &[
    left("aos", "aos", 24),
    left("los", "los", 24),
    right("aos_az [deg]", "aos_az_deg", 12),
    right("los_az [deg]", "los_az_deg", 12),
    left("tca_time", "tca_time", 24),
    right("tca_el [deg]", "tca_el_deg", 12),
    right("duration", "duration", 10),
];

type TransitRow = (
    Transit,
    Observation,
    Observation,
    DateTime<Utc>,
    Observation,
);

pub fn write_transits<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = anyhow::Result<TransitRow>>,
{
    let mut out = RowWriter::new(w, format, TRANSIT_COLUMNS);
    for item in iter {
        let (transit, aos_obs, los_obs, tca_time, tca_obs) = item?;
        out.write_row(&[
            time(transit.start),
            time(transit.end),
            num(aos_obs.azimuth.to_degrees().to_f64(), 2),
            num(los_obs.azimuth.to_degrees().to_f64(), 2),
            time(tca_time),
            num(tca_obs.elevation.to_degrees().to_f64(), 2),
            duration(transit.start, transit.end),
        ])?;
    }
    out.finish()
}

const GROUND_TRACK_COLUMNS: &[Column] = &[
    left("datetime", "datetime", 24),
    right("lat [deg]", "lat_deg", 10),
    right("lon [deg]", "lon_deg", 11),
    right("altitude [km]", "altitude_km", 14),
];

pub fn write_ground_track<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = PredictResult<(DateTime<Utc>, Geodetic)>>,
{
    let mut out = RowWriter::new(w, format, GROUND_TRACK_COLUMNS);
    for item in iter {
        let (t, point) = item.context("propagation error")?;
        out.write_row(&[
            time(t),
            num(point.latitude.to_f64(), 4),
            num(point.longitude.to_f64(), 4),
            num(point.altitude / 1_000.0, 3),
        ])?;
    }
    out.finish()
}

const AOI_COLUMNS: &[Column] = &[
    left("entry", "entry", 24),
    left("exit", "exit", 24),
    right("entry_lat [deg]", "entry_lat_deg", 15),
    right("entry_lon [deg]", "entry_lon_deg", 15),
    right("exit_lat [deg]", "exit_lat_deg", 14),
    right("exit_lon [deg]", "exit_lon_deg", 14),
    right("duration", "duration", 10),
];

/// The window plus the sub-satellite point at each end, which is where the
/// area came within reach and passed back out of it.
type AoiRow = (AoiWindow, Geodetic, Geodetic);

pub fn write_aoi<W, I>(w: W, format: Format, iter: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = anyhow::Result<AoiRow>>,
{
    let mut out = RowWriter::new(w, format, AOI_COLUMNS);
    for item in iter {
        let (window, entry, exit) = item?;
        out.write_row(&[
            time(window.start),
            time(window.end),
            num(entry.latitude.to_f64(), 4),
            num(entry.longitude.to_f64(), 4),
            num(exit.latitude.to_f64(), 4),
            num(exit.longitude.to_f64(), 4),
            duration(window.start, window.end),
        ])?;
    }
    out.finish()
}

const AOI_LIST_COLUMNS: &[Column] = &[
    left("id", "id", 16),
    left("shape", "shape", 8),
    left("definition", "definition", 64),
];

/// Render the config's AOIs, in id order.
///
/// `definition` uses the config file's own field names, so a listing reads the
/// same way the YAML does.
pub fn write_aois<'a, W, I>(w: W, format: Format, aois: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = (&'a str, &'a crate::config::AoiDef)>,
{
    let mut out = RowWriter::new(w, format, AOI_LIST_COLUMNS);
    for (id, def) in aois {
        out.write_row(&[text(id), text(def.kind()), text(def.describe())])?;
    }
    out.finish()
}

const GROUND_STATION_COLUMNS: &[Column] = &[
    left("id", "id", 16),
    right("latitude [deg]", "latitude", 14),
    right("longitude [deg]", "longitude", 15),
    right("altitude [m]", "altitude", 12),
];

/// Render the config's ground stations, in id order.
pub fn write_ground_stations<'a, W, I>(w: W, format: Format, stations: I) -> anyhow::Result<()>
where
    W: Write,
    I: Iterator<Item = (&'a str, &'a crate::config::GroundStation)>,
{
    let mut out = RowWriter::new(w, format, GROUND_STATION_COLUMNS);
    for (id, station) in stations {
        out.write_row(&[
            text(id),
            num(station.location.latitude, 4),
            num(station.location.longitude, 4),
            num(station.location.altitude, 1),
        ])?;
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: &[Column] = &[
        left("name", "name", 10),
        right("value [km]", "value_km", 12),
    ];

    fn render(format: Format, rows: &[&[Cell]]) -> String {
        let mut buf = Vec::new();
        {
            let mut out = RowWriter::new(&mut buf, format, COLUMNS);
            for row in rows {
                out.write_row(row).unwrap();
            }
            out.finish().unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_text_underline_matches_header_width() {
        let out = render(Format::Text, &[&[text("a"), num(1.0, 2)]]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0].chars().count(),
            lines[1].chars().count(),
            "header and underline disagree:\n{out}"
        );
        assert!(lines[1].chars().all(|c| c == '-'));
    }

    #[test]
    fn test_text_aligns_columns() {
        let out = render(Format::Text, &[&[text("abc"), num(1.5, 2)]]);
        let row = out.lines().nth(2).unwrap();
        assert!(row.starts_with("abc       "), "{row:?}");
        assert!(row.ends_with("1.50"), "{row:?}");
    }

    #[test]
    fn test_json_quotes_strings_but_not_numbers() {
        let out = render(Format::Json, &[&[text("abc"), num(1.5, 2)]]);
        assert_eq!(out, "{\"name\":\"abc\",\"value_km\":1.50}\n");
    }

    #[test]
    fn test_json_has_no_header() {
        let out = render(Format::Json, &[]);
        assert_eq!(out, "");
    }

    #[test]
    fn test_json_escapes_special_characters() {
        let out = render(Format::Json, &[&[text("a\"b\\c\nd"), num(0.0, 1)]]);
        assert!(out.contains(r#""name":"a\"b\\c\nd""#), "{out}");
    }

    #[test]
    fn test_csv_uses_keys_as_header() {
        let out = render(Format::Csv, &[&[text("abc"), num(1.5, 2)]]);
        assert_eq!(out, "name,value_km\nabc,1.50\n");
    }

    #[test]
    fn test_csv_quotes_embedded_commas_and_quotes() {
        let out = render(Format::Csv, &[&[text("a,b\"c"), num(1.0, 0)]]);
        assert_eq!(out.lines().nth(1).unwrap(), "\"a,b\"\"c\",1");
    }

    #[test]
    fn test_empty_result_still_writes_text_header() {
        let out = render(Format::Text, &[]);
        assert_eq!(out.lines().count(), 2, "{out:?}");
    }

    #[test]
    fn test_empty_result_still_writes_csv_header() {
        let out = render(Format::Csv, &[]);
        assert_eq!(out, "name,value_km\n");
    }

    #[test]
    fn test_text_truncates_over_wide_cells_to_keep_alignment() {
        let out = render(
            Format::Text,
            &[&[text("a-very-long-identifier"), num(1.0, 2)]],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1].chars().count(), lines[2].chars().count());
        assert!(lines[2].starts_with("a-very-lo…"), "{:?}", lines[2]);
    }

    /// CSV and JSON carry the full value; only the fixed-width table truncates.
    #[test]
    fn test_other_formats_do_not_truncate() {
        let row: &[Cell] = &[text("a-very-long-identifier"), num(1.0, 2)];
        assert!(render(Format::Csv, &[row]).contains("a-very-long-identifier"));
        assert!(render(Format::Json, &[row]).contains("a-very-long-identifier"));
    }

    #[test]
    fn test_non_finite_numbers_render_as_json_null() {
        let out = render(Format::Json, &[&[text("a"), num(f64::NAN, 2)]]);
        assert_eq!(out, "{\"name\":\"a\",\"value_km\":null}\n");
        let out = render(Format::Json, &[&[text("a"), num(f64::INFINITY, 2)]]);
        assert!(out.contains("\"value_km\":null"), "{out}");
    }

    /// Only JSON spells it `null`; text and CSV leave the field empty rather
    /// than printing the literal word.
    #[test]
    fn test_non_finite_numbers_are_blank_in_text_and_csv() {
        let row: &[Cell] = &[text("a"), num(f64::NAN, 2)];

        let csv = render(Format::Csv, &[row]);
        assert_eq!(csv.lines().nth(1).unwrap(), "a,");

        let out = render(Format::Text, &[row]);
        let data = out.lines().nth(2).unwrap();
        assert!(!data.contains("null"), "{data:?}");
        assert_eq!(data.trim(), "a");
    }

    #[test]
    fn test_duration_never_negative() {
        let t = Utc::now();
        assert_eq!(duration(t, t - chrono::Duration::seconds(5)).text(), "0s");
    }
}
