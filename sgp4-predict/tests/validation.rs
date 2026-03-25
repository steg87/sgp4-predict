use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::Deserialize;
use sgp4_predict::{HasId, HasTle, IlluminationState, Observation, Observer, Predictor, Transit};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// YAML spec structures
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TestVectors {
    tles: HashMap<String, Tle>,
    observers: HashMap<String, GroundStation>,
    test_cases: TestCases,
    #[serde(default)]
    benchmarks: Vec<BenchmarkTestCase>,
}

#[derive(Deserialize)]
struct TestCases {
    transits: Vec<TransitTestCase>,
    observations: Vec<ObservationTestCase>,
    #[serde(default)]
    illumination: Vec<IlluminationTestCase>,
}

#[derive(Deserialize)]
struct BenchmarkTestCase {
    name: String,
    transit_case: String,
    runs: Option<usize>,
}

#[derive(Deserialize)]
struct PyBenchmarkResult {
    #[allow(dead_code)]
    runs: usize,
    total_s: f64,
    avg_ms: f64,
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
    fn latitude_deg(&self) -> f64 {
        self.latitude_deg
    }
    fn longitude_deg(&self) -> f64 {
        self.longitude_deg
    }
    fn altitude(&self) -> f64 {
        self.altitude_m
    }
}

#[derive(Deserialize)]
struct TransitTestCase {
    name: String,
    tle: String,
    observer: String,
    start: Option<String>,
    duration_days: Option<f64>,
    min_elevation: Option<f64>,
    tolerances: TransitTolerances,
}

#[derive(Deserialize)]
struct ObservationTestCase {
    name: String,
    tle: String,
    observer: String,
    start: Option<String>,
    duration_days: Option<f64>,
    tolerances: ObservationTolerances,
    step_s: Option<f64>,
}

#[derive(Deserialize, Clone)]
struct TransitTolerances {
    aos_los_time_s: f64,
    azimuth_deg: f64,
    tca_elevation_deg: f64,
}

#[derive(Deserialize, Clone)]
struct ObservationTolerances {
    azimuth_deg: f64,
    elevation_deg: f64,
    range_km: f64,
}

#[derive(Deserialize)]
struct IlluminationTestCase {
    name: String,
    tle: String,
    start: Option<String>,
    duration_days: Option<f64>,
    step_s: Option<f64>,
    tolerances: IlluminationTolerances,
}

#[derive(Deserialize, Clone)]
struct IlluminationTolerances {
    max_mismatch_fraction: f64,
}

// ---------------------------------------------------------------------------
// CSV parsing
// ---------------------------------------------------------------------------

struct RefTransit {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    aos_az_deg: f64,
    los_az_deg: f64,
    tca_el_deg: f64,
}

fn parse_transit_csv(path: &Path) -> Vec<RefTransit> {
    let content: String = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    content
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.splitn(6, ',').collect();
            assert!(cols.len() >= 5, "unexpected CSV row: {line}");
            let parse_dt = |s: &str| -> DateTime<Utc> {
                let s = s.trim().trim_end_matches('Z');
                NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                    .unwrap_or_else(|_| panic!("bad datetime: {s}"))
                    .and_utc()
            };
            RefTransit {
                start: parse_dt(cols[0]),
                end: parse_dt(cols[1]),
                aos_az_deg: cols[2].trim().parse().unwrap(),
                los_az_deg: cols[3].trim().parse().unwrap(),
                tca_el_deg: cols[4].trim().parse().unwrap(),
            }
        })
        .collect()
}

struct RefObservation {
    time: DateTime<Utc>,
    az_deg: f64,
    el_deg: f64,
    range_km: f64,
}

fn parse_observation_csv(path: &Path) -> Vec<RefObservation> {
    let content: String = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    content
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.splitn(4, ',').collect();
            assert!(cols.len() >= 4, "unexpected observation CSV row: {line}");
            let raw: &str = cols[0].trim().trim_end_matches('Z');
            let time: DateTime<Utc> = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                .unwrap_or_else(|_| panic!("bad datetime: {}", cols[0]))
                .and_utc();
            RefObservation {
                time,
                az_deg: cols[1].trim().parse().unwrap(),
                el_deg: cols[2].trim().parse().unwrap(),
                range_km: cols[3].trim().parse().unwrap(),
            }
        })
        .collect()
}

struct RefIllumSample {
    time: DateTime<Utc>,
    state: IlluminationState,
}

fn parse_illumination_csv(path: &Path) -> Vec<RefIllumSample> {
    let content: String = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    content
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.splitn(3, ',').collect();
            assert!(cols.len() >= 2, "unexpected illumination CSV row: {line}");
            let raw: &str = cols[0].trim().trim_end_matches('Z');
            let time: DateTime<Utc> = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                .unwrap_or_else(|_| panic!("bad datetime: {}", cols[0]))
                .and_utc();
            let state: IlluminationState = match cols[1].trim() {
                "sunlit" => IlluminationState::Sunlit,
                "eclipse" => IlluminationState::Eclipse,
                other => panic!("unknown illumination state: {other}"),
            };
            RefIllumSample { time, state }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Stats collection
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TransitStats {
    aos_time_s: Vec<f64>,
    los_time_s: Vec<f64>,
    aos_az_deg: Vec<f64>,
    los_az_deg: Vec<f64>,
    tca_el_deg: Vec<f64>,
}

#[derive(Default)]
struct ObsStats {
    az_deg: Vec<f64>,
    el_deg: Vec<f64>,
    range_km: Vec<f64>,
}

fn avg(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

fn max(v: &[f64]) -> f64 {
    v.iter().cloned().fold(0.0_f64, f64::max)
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

struct TransitReport {
    name: String,
    tle_id: String,
    observer_id: String,
    window_start: DateTime<Utc>,
    duration_days: f64,
    transit_count_ours: usize,
    transit_count_ref: usize,
    transit_stats: TransitStats,
    transit_tolerances: TransitTolerances,
    errors: Vec<String>,
}

struct ObservationReport {
    name: String,
    tle_id: String,
    observer_id: String,
    window_start: DateTime<Utc>,
    duration_days: f64,
    stats: ObsStats,
    tolerances: ObservationTolerances,
    step_s: f64,
    errors: Vec<String>,
}

struct IlluminationReport {
    name: String,
    tle_id: String,
    window_start: DateTime<Utc>,
    duration_days: f64,
    step_s: f64,
    total_samples: usize,
    mismatch_count: usize,
    tolerances: IlluminationTolerances,
    errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Observation validation helper (shared by both report types)
// ---------------------------------------------------------------------------

fn validate_observations(
    p: &Predictor,
    gs: &impl Observer,
    sf_obs: &[RefObservation],
    tol: &ObservationTolerances,
    errors: &mut Vec<String>,
) -> ObsStats {
    let mut stats: ObsStats = ObsStats::default();
    for sfo in sf_obs {
        let obs: Observation = p
            .observe_at(sfo.time, gs)
            .unwrap_or_else(|e| panic!("observe_at {} failed: {e}", sfo.time));
        stats
            .az_deg
            .push((obs.azimuth.to_degrees() - sfo.az_deg).abs());
        stats
            .el_deg
            .push((obs.elevation.to_degrees() - sfo.el_deg).abs());
        stats
            .range_km
            .push((obs.range / 1_000.0 - sfo.range_km).abs());
    }
    let max_az: f64 = max(&stats.az_deg);
    let max_el: f64 = max(&stats.el_deg);
    let max_range: f64 = max(&stats.range_km);
    if max_az >= tol.azimuth_deg {
        errors.push(format!(
            "obs azimuth max diff {max_az:.4}° >= {}",
            tol.azimuth_deg
        ));
    }
    if max_el >= tol.elevation_deg {
        errors.push(format!(
            "obs elevation max diff {max_el:.4}° >= {}",
            tol.elevation_deg
        ));
    }
    if max_range >= tol.range_km {
        errors.push(format!(
            "obs range max diff {max_range:.4} km >= {}",
            tol.range_km
        ));
    }
    stats
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

fn write_obs_rows(out: &mut String, stats: &ObsStats, tol: &ObservationTolerances) {
    let obs_rows: &[(&str, &[f64], f64, &str)] = &[
        ("Obs azimuth (°)", &stats.az_deg, tol.azimuth_deg, "°"),
        ("Obs elevation (°)", &stats.el_deg, tol.elevation_deg, "°"),
        ("Obs range (km)", &stats.range_km, tol.range_km, "km"),
    ];
    for (label, vals, tol, unit) in obs_rows {
        let a: f64 = avg(vals);
        let m: f64 = max(vals);
        let status: &str = if m < *tol { "PASS" } else { "FAIL" };
        writeln!(
            out,
            "  {:<18} {:>8.4}{unit}  {:>8.4}{unit}  {:>8.4}{unit}  {}",
            label, a, m, tol, status
        )
        .unwrap();
    }
}

fn format_report(
    transit_cases: &[TransitReport],
    obs_cases: &[ObservationReport],
    illum_cases: &[IlluminationReport],
) -> String {
    let mut out: String = String::new();
    let width: usize = 68;
    let bar: String = "=".repeat(width);
    let thin: String = "-".repeat(width);

    writeln!(out, "{bar}").unwrap();
    writeln!(out, "  Validation Report").unwrap();
    writeln!(out, "{bar}").unwrap();

    let total: usize = transit_cases.len() + obs_cases.len() + illum_cases.len();
    let mut total_pass: usize = 0;

    for c in transit_cases {
        let case_pass: bool = c.errors.is_empty() && c.transit_count_ours == c.transit_count_ref;

        writeln!(out).unwrap();
        writeln!(out, "  Test case : {} [transit]", c.name).unwrap();
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
            "  Transits  : {} (pypredict: {})",
            c.transit_count_ours, c.transit_count_ref
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

        let tol: &TransitTolerances = &c.transit_tolerances;
        let ts: &TransitStats = &c.transit_stats;
        let rows: &[(&str, &[f64], f64, &str)] = &[
            ("AOS time (s)", &ts.aos_time_s, tol.aos_los_time_s, "s"),
            ("LOS time (s)", &ts.los_time_s, tol.aos_los_time_s, "s"),
            ("AOS azimuth (°)", &ts.aos_az_deg, tol.azimuth_deg, "°"),
            ("LOS azimuth (°)", &ts.los_az_deg, tol.azimuth_deg, "°"),
            (
                "TCA elevation (°)",
                &ts.tca_el_deg,
                tol.tca_elevation_deg,
                "°",
            ),
        ];
        for (label, vals, tol, unit) in rows {
            let a: f64 = avg(vals);
            let m: f64 = max(vals);
            let status: &str = if m < *tol { "PASS" } else { "FAIL" };
            writeln!(
                out,
                "  {:<18} {:>8.3}{unit}  {:>8.3}{unit}  {:>8.3}{unit}  {}",
                label, a, m, tol, status
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
        writeln!(out, "  Result: {}", if case_pass { "PASS" } else { "FAIL" }).unwrap();
        if case_pass {
            total_pass += 1;
        }
    }

    for c in obs_cases {
        let case_pass: bool = c.errors.is_empty();

        writeln!(out).unwrap();
        writeln!(out, "  Test case : {} [observation]", c.name).unwrap();
        writeln!(out, "  TLE       : {}", c.tle_id).unwrap();
        writeln!(out, "  Observer  : {}", c.observer_id).unwrap();
        writeln!(
            out,
            "  Window    : {}  +  {} days",
            c.window_start.format("%Y-%m-%d %H:%M:%S UTC"),
            c.duration_days
        )
        .unwrap();
        writeln!(out, "  Step      : {}s", c.step_s).unwrap();
        writeln!(out).unwrap();
        writeln!(out, "  {thin}").unwrap();
        writeln!(
            out,
            "  {:<18} {:>9}  {:>9}  {:>9}  Result",
            "Metric", "Avg", "Max", "Tol"
        )
        .unwrap();
        writeln!(out, "  {thin}").unwrap();
        write_obs_rows(&mut out, &c.stats, &c.tolerances);
        writeln!(out, "  {thin}").unwrap();
        if !c.errors.is_empty() {
            writeln!(out, "  Errors:").unwrap();
            for e in &c.errors {
                writeln!(out, "    • {e}").unwrap();
            }
        }
        writeln!(out, "  Result: {}", if case_pass { "PASS" } else { "FAIL" }).unwrap();
        if case_pass {
            total_pass += 1;
        }
    }

    for c in illum_cases {
        let mismatch_frac: f64 = if c.total_samples > 0 {
            c.mismatch_count as f64 / c.total_samples as f64
        } else {
            0.0
        };
        let case_pass: bool = c.errors.is_empty();

        writeln!(out).unwrap();
        writeln!(out, "  Test case : {} [illumination]", c.name).unwrap();
        writeln!(out, "  TLE       : {}", c.tle_id).unwrap();
        writeln!(
            out,
            "  Window    : {}  +  {} days",
            c.window_start.format("%Y-%m-%d %H:%M:%S UTC"),
            c.duration_days
        )
        .unwrap();
        writeln!(out, "  Step      : {}s", c.step_s).unwrap();
        writeln!(out).unwrap();
        writeln!(out, "  {thin}").unwrap();
        writeln!(
            out,
            "  {:<18} {:>9}  {:>9}  {:>9}  Result",
            "Metric", "Value", "", "Tol"
        )
        .unwrap();
        writeln!(out, "  {thin}").unwrap();
        let status: &str = if mismatch_frac < c.tolerances.max_mismatch_fraction {
            "PASS"
        } else {
            "FAIL"
        };
        writeln!(
            out,
            "  {:<18} {:>8}/{:<8}  {:>8.5}   {:.5}  {}",
            "Mismatch",
            c.mismatch_count,
            c.total_samples,
            mismatch_frac,
            c.tolerances.max_mismatch_fraction,
            status,
        )
        .unwrap();
        writeln!(out, "  {thin}").unwrap();
        if !c.errors.is_empty() {
            writeln!(out, "  Errors:").unwrap();
            for e in &c.errors {
                writeln!(out, "    • {e}").unwrap();
            }
        }
        writeln!(out, "  Result: {}", if case_pass { "PASS" } else { "FAIL" }).unwrap();
        if case_pass {
            total_pass += 1;
        }
    }

    writeln!(out).unwrap();
    writeln!(out, "{bar}").unwrap();
    writeln!(out, "  {total_pass}/{total} test case(s) passed").unwrap();
    writeln!(out, "{bar}").unwrap();

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_window(
    start: &Option<String>,
    duration_days: Option<f64>,
    p: &Predictor,
    name: &str,
) -> (DateTime<Utc>, f64, DateTime<Utc>) {
    let window_start: DateTime<Utc> = match start {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .unwrap_or_else(|e| panic!("bad start in '{name}': {e}"))
            .with_timezone(&Utc),
        None => p.epoch(),
    };
    let days: f64 = duration_days.unwrap_or(3.0);
    let window_end: DateTime<Utc> = window_start + Duration::seconds((days * 86_400.0) as i64);
    (window_start, days, window_end)
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
#[ignore = "slow — run with `make validation`"]
fn pypredict_validation() {
    let spec_path: &Path = Path::new("tests/data/test_vectors.yaml");
    let transits_dir: &Path = Path::new("tests/data/transits");
    let obs_dir: &Path = Path::new("tests/data/observations");
    let illum_dir: &Path = Path::new("tests/data/illumination");
    let report_path: &Path = Path::new("tests/data/validation_report.txt");

    // --- 1. Regenerate reference CSVs ---
    let py_output: std::process::Output = std::process::Command::new("uv")
        .args(["run", "tests/data/validation.py"])
        .output()
        .expect("failed to run uv — is uv installed?");
    assert!(
        py_output.status.success(),
        "pypredict: {}\n{}",
        String::from_utf8_lossy(&py_output.stdout),
        String::from_utf8_lossy(&py_output.stderr),
    );

    // --- 2. Parse spec ---
    let spec_text: String = std::fs::read_to_string(spec_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", spec_path.display()));
    let spec: TestVectors = serde_yaml::from_str(&spec_text)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", spec_path.display()));

    // --- 3. Validate transit test cases ---
    let mut transit_reports: Vec<TransitReport> = Vec::new();

    for tc in &spec.test_cases.transits {
        let tle: &Tle = spec
            .tles
            .get(&tc.tle)
            .unwrap_or_else(|| panic!("TLE '{}' not found in spec", tc.tle));
        let gs: &GroundStation = spec
            .observers
            .get(&tc.observer)
            .unwrap_or_else(|| panic!("observer '{}' not found in spec", tc.observer));

        let p: Predictor = Predictor::new(tle)
            .unwrap_or_else(|e| panic!("Predictor::new failed for '{}': {e}", tc.name));

        let (window_start, duration_days, window_end) =
            resolve_window(&tc.start, tc.duration_days, &p, &tc.name);

        let our_transits: Vec<Transit> = p
            .transits_iter(
                gs,
                window_start..window_end,
                tc.min_elevation.unwrap_or(0.0),
            )
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| panic!("transit iter error in '{}': {e}", tc.name));

        let csv_path: PathBuf = transits_dir.join(format!("{}.csv", tc.name));
        let sf_transits: Vec<RefTransit> = parse_transit_csv(&csv_path);

        let mut transit_stats: TransitStats = TransitStats::default();
        let mut errors: Vec<String> = Vec::new();
        let tol: &TransitTolerances = &tc.tolerances;

        if our_transits.len() != sf_transits.len() {
            errors.push(format!(
                "transit count mismatch: ours={} pypredict={}",
                our_transits.len(),
                sf_transits.len(),
            ));
        }

        for (i, (our, sf)) in our_transits.iter().zip(sf_transits.iter()).enumerate() {
            let ctx = || format!("transit {i} (AOS {})", our.start.format("%H:%M:%S"));

            let aos_diff: f64 = (our.start - sf.start).abs().num_milliseconds() as f64 / 1_000.0;
            let los_diff: f64 = (our.end - sf.end).abs().num_milliseconds() as f64 / 1_000.0;
            transit_stats.aos_time_s.push(aos_diff);
            transit_stats.los_time_s.push(los_diff);
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

            let aos_obs: Observation = p
                .observe_at(our.start, gs)
                .unwrap_or_else(|e| panic!("{}: observe_at AOS failed: {e}", ctx()));
            let aos_az_diff: f64 = (aos_obs.azimuth.to_degrees() - sf.aos_az_deg).abs();
            transit_stats.aos_az_deg.push(aos_az_diff);
            if aos_az_diff >= tol.azimuth_deg {
                errors.push(format!(
                    "{}: AOS az diff {aos_az_diff:.3}° >= {}",
                    ctx(),
                    tol.azimuth_deg
                ));
            }

            let los_obs: Observation = p
                .observe_at(our.end, gs)
                .unwrap_or_else(|e| panic!("{}: observe_at LOS failed: {e}", ctx()));
            let los_az_diff: f64 = (los_obs.azimuth.to_degrees() - sf.los_az_deg).abs();
            transit_stats.los_az_deg.push(los_az_diff);
            if los_az_diff >= tol.azimuth_deg {
                errors.push(format!(
                    "{}: LOS az diff {los_az_diff:.3}° >= {}",
                    ctx(),
                    tol.azimuth_deg
                ));
            }

            let (_, tca_obs): (DateTime<Utc>, Observation) = p
                .max_elevation(*our, gs)
                .unwrap_or_else(|e| panic!("{}: max_elevation failed: {e}", ctx()));
            let tca_el_diff: f64 = (tca_obs.elevation.to_degrees() - sf.tca_el_deg).abs();
            transit_stats.tca_el_deg.push(tca_el_diff);
            if tca_el_diff >= tol.tca_elevation_deg {
                errors.push(format!(
                    "{}: TCA el diff {tca_el_diff:.3}° >= {}",
                    ctx(),
                    tol.tca_elevation_deg
                ));
            }
        }

        transit_reports.push(TransitReport {
            name: tc.name.clone(),
            tle_id: tc.tle.clone(),
            observer_id: tc.observer.clone(),
            window_start,
            duration_days,
            transit_count_ours: our_transits.len(),
            transit_count_ref: sf_transits.len(),
            transit_stats,
            transit_tolerances: tol.clone(),
            errors,
        });
    }

    // --- 4. Validate observation-only test cases ---
    let mut obs_reports: Vec<ObservationReport> = Vec::new();

    for tc in &spec.test_cases.observations {
        let tle: &Tle = spec
            .tles
            .get(&tc.tle)
            .unwrap_or_else(|| panic!("TLE '{}' not found in spec", tc.tle));
        let gs: &GroundStation = spec
            .observers
            .get(&tc.observer)
            .unwrap_or_else(|| panic!("observer '{}' not found in spec", tc.observer));

        let p: Predictor = Predictor::new(tle)
            .unwrap_or_else(|e| panic!("Predictor::new failed for '{}': {e}", tc.name));

        let (window_start, duration_days, _) =
            resolve_window(&tc.start, tc.duration_days, &p, &tc.name);

        let step_s: f64 = tc.step_s.unwrap_or(60.0);
        let mut errors: Vec<String> = Vec::new();
        let obs_path: PathBuf = obs_dir.join(format!("{}.csv", tc.name));
        let sf_obs: Vec<RefObservation> = parse_observation_csv(&obs_path);
        let stats: ObsStats = validate_observations(&p, gs, &sf_obs, &tc.tolerances, &mut errors);

        obs_reports.push(ObservationReport {
            name: tc.name.clone(),
            tle_id: tc.tle.clone(),
            observer_id: tc.observer.clone(),
            window_start,
            duration_days,
            stats,
            tolerances: tc.tolerances.clone(),
            step_s,
            errors,
        });
    }

    // --- 5. Validate illumination test cases ---
    let mut illum_reports: Vec<IlluminationReport> = Vec::new();

    for tc in &spec.test_cases.illumination {
        let tle: &Tle = spec
            .tles
            .get(&tc.tle)
            .unwrap_or_else(|| panic!("TLE '{}' not found in spec", tc.tle));

        let p: Predictor = Predictor::new(tle)
            .unwrap_or_else(|e| panic!("Predictor::new failed for '{}': {e}", tc.name));

        let (window_start, duration_days, _) =
            resolve_window(&tc.start, tc.duration_days, &p, &tc.name);

        let step_s: f64 = tc.step_s.unwrap_or(60.0);
        let csv_path: PathBuf = illum_dir.join(format!("{}.csv", tc.name));
        let ref_samples: Vec<RefIllumSample> = parse_illumination_csv(&csv_path);

        let mut mismatch_count: usize = 0;
        let mut errors: Vec<String> = Vec::new();

        for sample in &ref_samples {
            let our_state: IlluminationState = p
                .illumination_state(sample.time)
                .unwrap_or_else(|e| panic!("illumination_state failed at {}: {e}", sample.time));
            if our_state != sample.state {
                mismatch_count += 1;
            }
        }

        let total_samples: usize = ref_samples.len();
        let mismatch_frac: f64 = if total_samples > 0 {
            mismatch_count as f64 / total_samples as f64
        } else {
            0.0
        };
        if mismatch_frac >= tc.tolerances.max_mismatch_fraction {
            errors.push(format!(
                "mismatch fraction {mismatch_frac:.5} >= {} ({mismatch_count}/{total_samples} samples)",
                tc.tolerances.max_mismatch_fraction,
            ));
        }

        illum_reports.push(IlluminationReport {
            name: tc.name.clone(),
            tle_id: tc.tle.clone(),
            window_start,
            duration_days,
            step_s,
            total_samples,
            mismatch_count,
            tolerances: tc.tolerances.clone(),
            errors,
        });
    }

    // --- 6. Write and print report ---
    let report: String = format_report(&transit_reports, &obs_reports, &illum_reports);
    std::fs::write(report_path, &report)
        .unwrap_or_else(|e| eprintln!("warning: could not write report: {e}"));
    println!("{report}");

    // --- 7. Fail if any errors ---
    let all_errors: Vec<String> = transit_reports
        .iter()
        .map(|r| (r.name.as_str(), &r.errors))
        .chain(obs_reports.iter().map(|r| (r.name.as_str(), &r.errors)))
        .chain(illum_reports.iter().map(|r| (r.name.as_str(), &r.errors)))
        .flat_map(|(name, errs)| errs.iter().map(move |e| format!("[{name}] {e}")))
        .collect();

    assert!(
        all_errors.is_empty(),
        "validation failed:\n{}",
        all_errors.join("\n"),
    );
}

#[test]
#[ignore = "slow — run with `make validation`"]
fn montecarlo_benchmark() {
    let spec_path = Path::new("tests/data/test_vectors.yaml");
    let results_path = Path::new("tests/data/benchmark_results.json");
    let report_path = Path::new("tests/data/benchmark_report.txt");

    // 1. Parse spec
    let spec_text = std::fs::read_to_string(spec_path).unwrap();
    let spec: TestVectors = serde_yaml::from_str(&spec_text).unwrap();
    if spec.benchmarks.is_empty() {
        return;
    }

    // 2. Run Python benchmark
    let py_out = std::process::Command::new("uv")
        .args(["run", "tests/data/validation.py", "--benchmark"])
        .output()
        .expect("failed to run uv");
    assert!(
        py_out.status.success(),
        "Python benchmark failed:\n{}",
        String::from_utf8_lossy(&py_out.stderr)
    );

    // 3. Read Python results (JSON is valid YAML, so serde_yaml can parse it)
    let json_text = std::fs::read_to_string(results_path).unwrap();
    let py_results: HashMap<String, PyBenchmarkResult> = serde_yaml::from_str(&json_text).unwrap();

    // 4. Run Rust benchmarks and build report
    let mut out = String::new();
    let width = 68;
    let bar = "=".repeat(width);
    writeln!(out, "{bar}").unwrap();
    writeln!(out, "  Monte Carlo Benchmark: Rust vs pypredict").unwrap();
    writeln!(out, "{bar}").unwrap();

    for bc in &spec.benchmarks {
        let runs = bc.runs.unwrap_or(1000);
        let tc = spec
            .test_cases
            .transits
            .iter()
            .find(|t| t.name == bc.transit_case)
            .unwrap_or_else(|| panic!("transit case '{}' not found", bc.transit_case));
        let tle = spec.tles.get(&tc.tle).unwrap();
        let gs = spec.observers.get(&tc.observer).unwrap();
        let p = Predictor::new(tle).unwrap();
        let (window_start, _, window_end) =
            resolve_window(&tc.start, tc.duration_days, &p, &tc.name);
        let min_el = tc.min_elevation.unwrap_or(0.0);

        let t0 = std::time::Instant::now();
        for _ in 0..runs {
            p.transits_iter(gs, window_start..window_end, min_el)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        }
        let rust_total_s = t0.elapsed().as_secs_f64();
        let rust_avg_ms = rust_total_s / runs as f64 * 1000.0;

        let py = py_results
            .get(&bc.name)
            .unwrap_or_else(|| panic!("no Python result for '{}'", bc.name));
        let speedup_x = py.avg_ms / rust_avg_ms;

        writeln!(out).unwrap();
        writeln!(out, "  Benchmark : {}", bc.name).unwrap();
        writeln!(out, "  Case      : {} ({} runs)", bc.transit_case, runs).unwrap();
        writeln!(
            out,
            "  {:>10}  {:>12}  {:>12}",
            "Impl", "Total (ms)", "Avg (ms)"
        )
        .unwrap();
        writeln!(out, "  {}", "-".repeat(40)).unwrap();
        writeln!(
            out,
            "  {:>10}  {:>12.1}  {:>12.3}",
            "Rust",
            rust_total_s * 1000.0,
            rust_avg_ms
        )
        .unwrap();
        writeln!(
            out,
            "  {:>10}  {:>12.1}  {:>12.3}",
            "pypredict",
            py.total_s * 1000.0,
            py.avg_ms
        )
        .unwrap();
        writeln!(out, "  {}", "-".repeat(40)).unwrap();
        writeln!(out, "  Rust is {speedup_x:.1}x faster than pypredict").unwrap();
    }

    writeln!(out).unwrap();
    writeln!(out, "{bar}").unwrap();
    std::fs::write(report_path, &out).ok();
    println!("{out}");
}
