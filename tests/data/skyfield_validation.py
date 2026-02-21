# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "skyfield",
#   "pyyaml",
# ]
# ///
"""
Generate transit reference CSVs for every test case in test_vectors.yaml using
skyfield, matching the format produced by the Rust test_transits_to_csv test.

Usage:
    uv run tests/data/skyfield_validation.py

One CSV is written per test case to tests/data/transits/{name}.csv.
"""

import csv
from datetime import datetime, timedelta, timezone
from pathlib import Path

import yaml
from skyfield.api import EarthSatellite, load, wgs84

HERE = Path(__file__).parent
SPEC_FILE = HERE / "test_vectors.yaml"
OUTPUT_DIR = HERE / "transits"


def signed_az(az_deg: float) -> float:
    """Convert azimuth from [0, 360) to (-180, 180] to match atan2 convention."""
    return az_deg if az_deg <= 180.0 else az_deg - 360.0


def format_duration(total_seconds: float) -> str:
    """Format a duration as humantime does: '14m 27s', '6m', '1h 2m 3s', etc."""
    s = int(round(total_seconds))
    h, rem = divmod(s, 3600)
    m, s = divmod(rem, 60)
    parts = []
    if h:
        parts.append(f"{h}h")
    if m:
        parts.append(f"{m}m")
    if s:
        parts.append(f"{s}s")
    return " ".join(parts) if parts else "0s"


def fmt_dt(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%d %H:%M:%S")


def parse_utc(s: str) -> datetime:
    """Parse an RFC 3339 / ISO 8601 string to a timezone-aware UTC datetime."""
    s = s.strip()
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return datetime.fromisoformat(s).astimezone(timezone.utc)


def collect_passes(sat, observer, t0, t1) -> list[tuple]:
    """Return (aos, tca, los) skyfield Time triples for all complete passes."""
    times, events = sat.find_events(observer, t0, t1, altitude_degrees=0.0)
    passes = []
    i = 0
    while i < len(events):
        if (
            events[i] == 0
            and i + 2 < len(events)
            and events[i + 1] == 1
            and events[i + 2] == 2
        ):
            passes.append((times[i], times[i + 1], times[i + 2]))
            i += 3
        else:
            i += 1
    return passes


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    spec = yaml.safe_load(SPEC_FILE.read_text())
    tles = spec["tles"]
    observers = spec["observers"]

    ts = load.timescale()

    for tc in spec["test_cases"]:
        name = tc["name"]
        tle = tles[tc["tle"]]
        observer = observers[tc["observer"]]

        sat = EarthSatellite(
            tle["line_1"], tle["line_2"], tle["name"], ts
        )
        observer = wgs84.latlon(
            observer["latitude_deg"],
            observer["longitude_deg"],
            elevation_m=observer["altitude_m"],
        )

        if tc.get("start"):
            start_dt = parse_utc(tc["start"])
        else:
            start_dt = sat.epoch.utc_datetime()

        duration_days = tc.get("duration_days", 3)
        end_dt = start_dt + timedelta(days=duration_days)

        t0 = ts.from_datetime(start_dt)
        t1 = ts.from_datetime(end_dt)
        passes = collect_passes(sat, observer, t0, t1)

        out_path = OUTPUT_DIR / f"{name}.csv"
        with open(out_path, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(
                [
                    "start",
                    "end",
                    "aos_azimuth_deg",
                    "los_azimuth_deg",
                    "tca_elevation_deg",
                    "duration",
                ]
            )
            for t_aos, t_tca, t_los in passes:
                diff = sat - observer
                _, aos_az, _ = diff.at(t_aos).altaz()
                tca_el, _, _ = diff.at(t_tca).altaz()
                _, los_az, _ = diff.at(t_los).altaz()

                aos_dt = t_aos.utc_datetime().replace(tzinfo=None, microsecond=0)
                los_dt = t_los.utc_datetime().replace(tzinfo=None, microsecond=0)
                duration_s = (
                    t_los.utc_datetime() - t_aos.utc_datetime()
                ).total_seconds()

                writer.writerow(
                    [
                        fmt_dt(aos_dt),
                        fmt_dt(los_dt),
                        f"{signed_az(aos_az.degrees):.2f}",
                        f"{signed_az(los_az.degrees):.2f}",
                        f"{tca_el.degrees:.2f}",
                        format_duration(duration_s),
                    ]
                )

        print(f"[{name}] wrote {len(passes)} transits → {out_path.name}")


if __name__ == "__main__":
    main()
