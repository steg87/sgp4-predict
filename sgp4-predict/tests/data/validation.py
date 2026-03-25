# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "pypredict",
#   "pyyaml",
#   "skyfield",
# ]
# ///
"""
Generate transit, observation, and illumination reference CSVs for every test
case in test_vectors.yaml using pypredict (transits/observations) and skyfield
(illumination).

Usage:
    # Output validation test results
    uv run tests/data/validation.py

    # Benchmark performance
    uv run tests/data/validation.py --benchmarks


Transit CSVs are written to tests/data/transits/{name}.csv.
Observation CSVs are written to tests/data/observations/{name}.csv.
Illumination CSVs are written to tests/data/illumination/{name}.csv.
"""

import argparse
import csv
import json
import time
from collections.abc import Iterator
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

import predict
import yaml
from skyfield.api import EarthSatellite, Loader

HERE = Path(__file__).parent
SPEC_FILE = HERE / "test_vectors.yaml"
OUTPUT_DIR = HERE / "transits"
OBS_DIR = HERE / "observations"
ILLUM_DIR = HERE / "illumination"
DATETIME_FMT = "%Y-%m-%dT%H:%M:%S.%fZ"


def build_sat(tle: dict) -> list[str]:
    return [tle["name"], tle["line_1"], tle["line_2"]]


def build_qth(observer: dict) -> tuple:
    return (
        observer["latitude_deg"],
        -observer["longitude_deg"],
        observer["altitude_m"],
    )


def parse_tle_epoch(line_1: str) -> datetime:
    """Parse TLE epoch from line 1 columns 18:32 (YYDDD.DDDDDDDD)."""
    epoch_str = line_1[18:32].strip()
    yy = int(epoch_str[:2])
    day_fraction = float(epoch_str[2:])
    year = 2000 + yy if yy < 57 else 1900 + yy
    epoch = datetime(year, 1, 1, tzinfo=timezone.utc) + timedelta(days=day_fraction - 1)
    return epoch


def resolve_window(tc: dict, tle: dict) -> tuple[datetime, datetime]:
    if "start" in tc:
        start = datetime.fromisoformat(tc["start"].replace("Z", "+00:00"))
    else:
        start = parse_tle_epoch(tle["line_1"])
    end = start + timedelta(days=tc.get("duration_days", 3.0))
    return start, end


def signed_az(az_deg: float) -> float:
    """Convert azimuth from [0, 360) to (-180, 180] to match atan2 convention."""
    return az_deg if az_deg <= 180.0 else az_deg - 360.0


@dataclass(frozen=True)
class Transit:
    start: datetime
    end: datetime
    azimuth_aos_deg: float
    azimuth_los_deg: float
    elevation_tca_deg: float

    def write(self, writer: csv.writer) -> None:
        start_iso = self.start.strftime(DATETIME_FMT)
        end_iso = self.end.strftime(DATETIME_FMT)
        writer.writerow(
            [
                start_iso,
                end_iso,
                signed_az(self.azimuth_aos_deg),
                signed_az(self.azimuth_los_deg),
                self.elevation_tca_deg,
            ]
        )


@dataclass(frozen=True)
class Observation:
    time: datetime
    azimuth_deg: float
    elevation_deg: float
    range_km: float

    def write(self, writer: csv.writer) -> None:
        time_iso = self.time.strftime(DATETIME_FMT)
        writer.writerow(
            [
                time_iso,
                signed_az(self.azimuth_deg),
                self.elevation_deg,
                self.range_km,
            ]
        )


@dataclass(frozen=True)
class IllumSample:
    time: datetime
    state: str  # "sunlit" | "eclipse"

    def write(self, writer: csv.writer) -> None:
        writer.writerow([self.time.strftime(DATETIME_FMT), self.state])


def generate_transits(
    tle_lines: list[str],
    qth: tuple,
    start: datetime,
    end: datetime,
    min_elevation: float = 0.0,
) -> Iterator[Transit]:
    for transit in predict.transits(
        tle_lines,
        qth,
        ending_after=start.timestamp(),
        ending_before=end.timestamp(),
    ):
        peak = transit.peak()
        if peak["elevation"] < min_elevation:
            continue
        pruned = transit.above(min_elevation)
        aos_time = datetime.fromtimestamp(pruned.start, tz=timezone.utc)
        los_time = datetime.fromtimestamp(pruned.end, tz=timezone.utc)
        aos_obs = predict.observe(tle_lines, qth, at=pruned.start)
        los_obs = predict.observe(tle_lines, qth, at=pruned.end)
        yield Transit(
            start=aos_time,
            end=los_time,
            azimuth_aos_deg=aos_obs["azimuth"],
            azimuth_los_deg=los_obs["azimuth"],
            elevation_tca_deg=peak["elevation"],
        )


def generate_observations(
    tle_lines: list[str],
    qth: tuple,
    start: datetime,
    end: datetime,
    step_s: float = 60.0,
) -> Iterator[Observation]:
    t = start
    while t < end:
        obs = predict.observe(tle_lines, qth, at=t.timestamp())
        yield Observation(
            time=t,
            azimuth_deg=obs["azimuth"],
            elevation_deg=obs["elevation"],
            range_km=obs["slant_range"],
        )
        t += timedelta(seconds=step_s)


def generate_illumination(
    tle: dict,
    start: datetime,
    end: datetime,
    step_s: float = 60.0,
) -> Iterator[IllumSample]:

    sky_load = Loader(str(HERE))
    ts = sky_load.timescale()
    eph = sky_load("de421.bsp")
    sat = EarthSatellite(tle["line_1"], tle["line_2"], tle["name"], ts)
    t = start
    while t < end:
        sky_t = ts.from_datetime(t)
        state = "sunlit" if sat.at(sky_t).is_sunlit(eph) else "eclipse"
        yield IllumSample(t, state)
        t += timedelta(seconds=step_s)


def run_benchmarks(spec: dict) -> None:
    results = {}
    for bc in spec.get("benchmarks", []):
        transit_tc = next(
            tc
            for tc in spec["test_cases"]["transits"]
            if tc["name"] == bc["transit_case"]
        )
        runs = bc.get("runs", 1000)
        tle = spec["tles"][transit_tc["tle"]]
        qth = build_qth(spec["observers"][transit_tc["observer"]])
        tle_lines = build_sat(tle)
        start, end = resolve_window(transit_tc, tle)
        min_el = transit_tc.get("min_elevation", 0.0)

        t0 = time.monotonic()
        for _ in range(runs):
            list(generate_transits(tle_lines, qth, start, end, min_el))
        total_s = time.monotonic() - t0

        results[bc["name"]] = {
            "runs": runs,
            "total_s": total_s,
            "avg_ms": total_s / runs * 1000.0,
        }

    out_path = HERE / "benchmark_results.json"
    out_path.write_text(json.dumps(results, indent=2))
    print(f"Benchmark results written to {out_path}")


def run_validation(spec: dict) -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    OBS_DIR.mkdir(parents=True, exist_ok=True)
    ILLUM_DIR.mkdir(parents=True, exist_ok=True)

    tles = spec["tles"]
    observers = spec["observers"]
    test_cases = spec["test_cases"]

    for tc in test_cases["transits"]:
        tle = tles[tc["tle"]]
        qth = build_qth(observers[tc["observer"]])
        tle_lines = build_sat(tle)
        start, end = resolve_window(tc, tle)
        min_el = tc.get("min_elevation", 0.0)
        with open(OUTPUT_DIR / f"{tc['name']}.csv", "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(
                [
                    "start_time",
                    "end_time",
                    "aos_azimuth_deg",
                    "los_azimuth_deg",
                    "tca_elevation_deg",
                ]
            )
            for t in generate_transits(tle_lines, qth, start, end, min_el):
                t.write(writer)

    for tc in test_cases["observations"]:
        tle = tles[tc["tle"]]
        qth = build_qth(observers[tc["observer"]])
        tle_lines = build_sat(tle)
        start, end = resolve_window(tc, tle)
        step_s = tc.get("step_s", 60.0)
        with open(OBS_DIR / f"{tc['name']}.csv", "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["time", "azimuth_deg", "elevation_deg", "range_km"])
            for obs in generate_observations(tle_lines, qth, start, end, step_s):
                obs.write(writer)

    for tc in test_cases.get("illumination", []):
        tle = tles[tc["tle"]]
        start, end = resolve_window(tc, tle)
        step_s = tc.get("step_s", 60.0)
        with open(ILLUM_DIR / f"{tc['name']}.csv", "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["time", "state"])
            for sample in generate_illumination(tle, start, end, step_s):
                sample.write(writer)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--benchmark", action="store_true")
    args = parser.parse_args()
    spec = yaml.safe_load(SPEC_FILE.read_text())
    if args.benchmark:
        run_benchmarks(spec)
    else:
        run_validation(spec)
