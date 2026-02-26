use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::Deserialize;
use sgp4_predict::{HasId, HasTle, Observer, Predictor};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::Path;

// ---------------------------------------------------------------------------
// YAML spec structures
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TestVectors {
    tles: HashMap<String, Tle>,
    observers: HashMap<String, GroundStation>,
    test_cases: Vec<TestCase>,
}

#[derive(Deserialize)]
struct Tle {
    name: String,
    line_1: String,
    line_2: String,
}

impl HasId for Tle {
    fn id(&self) -> &str {
        &self.name
    }
}

impl HasTle for Tle {
    fn line_1(&self) -> &str {
        &self.line_1
    }
    fn line_2(&self) -> &str {
        &self.line_2
    }
}

#[derive(Deserialize)]
struct GroundStation {
    latitude_deg: f64,
    longitude_deg: f64,
    altitude_m: f64,
}

impl Observer for GroundStation {
    fn latitude(&self) -> f64 {
        self.latitude_deg.to_radians()
    }
    fn longitude(&self) -> f64 {
        self.longitude_deg.to_radians()
    }
    fn altitude(&self) -> f64 {
        self.altitude_m
    }
}

#[derive(Deserialize)]
struct TestCase {
    name: String,
    tle: String,
    observer: String,
    start: Option<String>,
    duration_days: Option<f64>,
    tolerances: Tolerances,
}

#[derive(Deserialize, Clone)]
struct Tolerances {
    aos_los_time_s: f64,
    azimuth_deg: f64,
    tca_elevation_deg: f64,
}

// ---------------------------------------------------------------------------
// CSV parsing
// ---------------------------------------------------------------------------

struct SkyFieldTransit {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    aos_az_deg: f64,
    los_az_deg: f64,
    tca_el_deg: f64,
}

fn parse_skyfield_csv(path: &Path) -> Vec<SkyFieldTransit> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    content
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.splitn(6, ',').collect();
            assert!(cols.len() >= 5, "unexpected CSV row: {line}");
            let parse_dt = |s: &str| {
                NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
                    .unwrap_or_else(|_| panic!("bad datetime: {s}"))
                    .and_utc()
            };
            SkyFieldTransit {
                start: parse_dt(cols[0]),
                end: parse_dt(cols[1]),
                aos_az_deg: cols[2].trim().parse().unwrap(),
                los_az_deg: cols[3].trim().parse().unwrap(),
                tca_el_deg: cols[4].trim().parse().unwrap(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Stats collection
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CaseStats {
    aos_time_s: Vec<f64>,
    los_time_s: Vec<f64>,
    aos_az_deg: Vec<f64>,
    los_az_deg: Vec<f64>,
    tca_el_deg: Vec<f64>,
}

impl CaseStats {
    fn avg(v: &[f64]) -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        v.iter().sum::<f64>() / v.len() as f64
    }
    fn max(v: &[f64]) -> f64 {
        v.iter().cloned().fold(0.0_f64, f64::max)
    }
}

struct CaseReport {
    name: String,
    tle_id: String,
    observer_id: String,
    window_start: DateTime<Utc>,
    duration_days: f64,
    transit_count_ours: usize,
    transit_count_sf: usize,
    stats: CaseStats,
    errors: Vec<String>,
    tolerances: Tolerances,
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

fn format_report(cases: &[CaseReport]) -> String {
    let mut out = String::new();
    let width = 68;
    let bar = "=".repeat(width);
    let thin = "-".repeat(width);

    writeln!(out, "{bar}").unwrap();
    writeln!(out, "  Validation Report").unwrap();
    writeln!(out, "{bar}").unwrap();

    let mut total_pass = 0usize;

    for c in cases {
        let case_pass = c.errors.is_empty() && c.transit_count_ours == c.transit_count_sf;

        writeln!(out).unwrap();
        writeln!(out, "  Test case : {}", c.name).unwrap();
        writeln!(out, "  TLE       : {}", c.tle_id).unwrap();
        writeln!(out, "  Observer  : {}", c.observer_id).unwrap();
        writeln!(
            out,
            "  Window    : {}  +  {} days",
            c.window_start.format("%Y-%m-%d %H:%M:%S UTC"),
            c.duration_days
        )
        .unwrap();
        writeln!(
            out,
            "  Transits  : {} (skyfield: {})",
            c.transit_count_ours, c.transit_count_sf
        )
        .unwrap();
        writeln!(out).unwrap();

        writeln!(out, "  {thin}").unwrap();
        writeln!(
            out,
            "  {:<18} {:>9}  {:>9}  {:>9}  Result",
            "Metric", "Avg", "Max", "Tol"
        )
        .unwrap();
        writeln!(out, "  {thin}").unwrap();

        let rows: &[(&str, &[f64], f64, &str)] = &[
            (
                "AOS time (s)",
                &c.stats.aos_time_s,
                c.tolerances.aos_los_time_s,
                "s",
            ),
            (
                "LOS time (s)",
                &c.stats.los_time_s,
                c.tolerances.aos_los_time_s,
                "s",
            ),
            (
                "AOS azimuth (°)",
                &c.stats.aos_az_deg,
                c.tolerances.azimuth_deg,
                "°",
            ),
            (
                "LOS azimuth (°)",
                &c.stats.los_az_deg,
                c.tolerances.azimuth_deg,
                "°",
            ),
            (
                "TCA elevation (°)",
                &c.stats.tca_el_deg,
                c.tolerances.tca_elevation_deg,
                "°",
            ),
        ];

        for (label, vals, tol, unit) in rows {
            let avg = CaseStats::avg(vals);
            let max = CaseStats::max(vals);
            let status = if max < *tol { "PASS" } else { "FAIL" };
            writeln!(
                out,
                "  {:<18} {:>8.3}{unit}  {:>8.3}{unit}  {:>8.3}{unit}  {}",
                label, avg, max, tol, status
            )
            .unwrap();
        }

        writeln!(out, "  {thin}").unwrap();

        if !c.errors.is_empty() {
            writeln!(out, "  Errors:").unwrap();
            for e in &c.errors {
                writeln!(out, "    • {e}").unwrap();
            }
        }

        let result = if case_pass { "PASS" } else { "FAIL" };
        writeln!(out, "  Result: {result}").unwrap();

        if case_pass {
            total_pass += 1;
        }
    }

    writeln!(out).unwrap();
    writeln!(out, "{bar}").unwrap();
    writeln!(out, "  {total_pass}/{} test case(s) passed", cases.len()).unwrap();
    writeln!(out, "{bar}").unwrap();

    out
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn skyfield_validation() {
    let spec_path = Path::new("tests/data/test_vectors.yaml");
    let transits_dir = Path::new("tests/data/transits");
    let report_path = Path::new("tests/data/validation_report.txt");

    // --- 1. Regenerate skyfield reference CSVs ---
    let py_output = std::process::Command::new("uv")
        .args(["run", "tests/data/skyfield_validation.py"])
        .output()
        .expect("failed to run uv — is uv installed?");
    assert!(
        py_output.status.success(),
        "skyfield_validation.py failed:\n{}",
        String::from_utf8_lossy(&py_output.stderr),
    );

    // --- 2. Parse spec ---
    let spec_text = std::fs::read_to_string(spec_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", spec_path.display()));
    let spec: TestVectors = serde_yaml::from_str(&spec_text)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", spec_path.display()));

    // --- 3. Validate each test case ---
    let mut reports: Vec<CaseReport> = Vec::new();

    for tc in &spec.test_cases {
        let tle = spec
            .tles
            .get(&tc.tle)
            .unwrap_or_else(|| panic!("TLE '{}' not found in spec", tc.tle));
        let gs = spec
            .observers
            .get(&tc.observer)
            .unwrap_or_else(|| panic!("observer '{}' not found in spec", tc.observer));

        let p = Predictor::new(tle)
            .unwrap_or_else(|e| panic!("Predictor::new failed for '{}': {e}", tc.name));

        let window_start = match &tc.start {
            Some(s) => DateTime::parse_from_rfc3339(s)
                .unwrap_or_else(|e| panic!("bad start in '{}': {e}", tc.name))
                .with_timezone(&Utc),
            None => p.epoch(),
        };
        let duration_days = tc.duration_days.unwrap_or(3.0);
        let window_end = window_start + Duration::seconds((duration_days * 86_400.0) as i64);

        // Our transits
        let our_transits: Vec<_> = p
            .transits_iter(gs, window_start..window_end, 0.0)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| panic!("transit iter error in '{}': {e}", tc.name));

        // Skyfield reference
        let csv_path = transits_dir.join(format!("{}.csv", tc.name));
        let sf_transits = parse_skyfield_csv(&csv_path);

        // Collect stats and errors
        let mut stats = CaseStats::default();
        let mut errors: Vec<String> = Vec::new();

        let tol = &tc.tolerances;

        if our_transits.len() != sf_transits.len() {
            errors.push(format!(
                "transit count mismatch: ours={} skyfield={}",
                our_transits.len(),
                sf_transits.len(),
            ));
        }

        for (i, (our, sf)) in our_transits.iter().zip(sf_transits.iter()).enumerate() {
            let ctx = || format!("transit {i} (AOS {})", our.start.format("%H:%M:%S"));

            let aos_diff = (our.start - sf.start).abs().num_milliseconds() as f64 / 1_000.0;
            let los_diff = (our.end - sf.end).abs().num_milliseconds() as f64 / 1_000.0;
            stats.aos_time_s.push(aos_diff);
            stats.los_time_s.push(los_diff);
            if aos_diff > tol.aos_los_time_s {
                errors.push(format!(
                    "{}: AOS time diff {aos_diff:.3}s > {}",
                    ctx(),
                    tol.aos_los_time_s
                ));
            }
            if los_diff > tol.aos_los_time_s {
                errors.push(format!(
                    "{}: LOS time diff {los_diff:.3}s > {}",
                    ctx(),
                    tol.aos_los_time_s
                ));
            }

            let aos_obs = p
                .observe_at(our.start, gs)
                .unwrap_or_else(|e| panic!("{}: observe_at AOS failed: {e}", ctx()));
            let aos_az = aos_obs.azimuth.to_degrees();
            let aos_az_diff = (aos_az - sf.aos_az_deg).abs();
            stats.aos_az_deg.push(aos_az_diff);
            if aos_az_diff >= tol.azimuth_deg {
                errors.push(format!(
                    "{}: AOS az diff {aos_az_diff:.3}° >= {}",
                    ctx(),
                    tol.azimuth_deg
                ));
            }

            let los_obs = p
                .observe_at(our.end, gs)
                .unwrap_or_else(|e| panic!("{}: observe_at LOS failed: {e}", ctx()));
            let los_az = los_obs.azimuth.to_degrees();
            let los_az_diff = (los_az - sf.los_az_deg).abs();
            stats.los_az_deg.push(los_az_diff);
            if los_az_diff >= tol.azimuth_deg {
                errors.push(format!(
                    "{}: LOS az diff {los_az_diff:.3}° >= {}",
                    ctx(),
                    tol.azimuth_deg
                ));
            }

            let (_, tca_obs) = p
                .max_elevation(*our, gs)
                .unwrap_or_else(|e| panic!("{}: max_elevation failed: {e}", ctx()));
            let tca_el = tca_obs.elevation.to_degrees();
            let tca_el_diff = (tca_el - sf.tca_el_deg).abs();
            stats.tca_el_deg.push(tca_el_diff);
            if tca_el_diff >= tol.tca_elevation_deg {
                errors.push(format!(
                    "{}: TCA el diff {tca_el_diff:.3}° >= {}",
                    ctx(),
                    tol.tca_elevation_deg
                ));
            }
        }

        reports.push(CaseReport {
            name: tc.name.clone(),
            tle_id: tc.tle.clone(),
            observer_id: tc.observer.clone(),
            window_start,
            duration_days,
            transit_count_ours: our_transits.len(),
            transit_count_sf: sf_transits.len(),
            stats,
            errors,
            tolerances: tol.clone(),
        });
    }

    // --- 4. Write and print report ---
    let report = format_report(&reports);
    std::fs::write(report_path, &report)
        .unwrap_or_else(|e| eprintln!("warning: could not write report: {e}"));
    println!("{report}");

    // --- 5. Fail if any errors ---
    let all_errors: Vec<String> = reports
        .iter()
        .flat_map(|r| r.errors.iter().map(|e| format!("[{}] {e}", r.name)))
        .collect();

    assert!(
        all_errors.is_empty(),
        "validation failed:\n{}",
        all_errors.join("\n"),
    );
}
