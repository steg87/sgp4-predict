# sgp4-predict-cli

[![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict-cli)](../LICENSE-MIT)

A command-line tool for SGP4 satellite pass prediction. Provides tabular output for transits, observations, state vectors, apsides, and illumination windows.

## Installation

```sh
cargo install sgp4-predict-cli
```

## TLE input

All subcommands accept a TLE via `--tle-file`. If omitted, the TLE is read from stdin.

Fresh TLEs can be obtained from [CelesTrak](https://celestrak.org). SGP4 accuracy degrades with TLE age — for LEO satellites, treat TLEs older than 3–7 days with caution. The tool will warn if the TLE epoch is stale.

Either input takes the same 2-line or 3-line text. With three lines the first is the satellite name; with two, the name is derived from the NORAD id in line 1 (`NORAD-60989`). Blank lines and surrounding whitespace are ignored.

```
SENTINEL-2C
1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740
```

**From a file:**

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow
```

**Piped in** — anything that writes a TLE to stdout works, so TLEs can be fetched and predicted in one step:

```sh
cat sentinel.tle | sgp4-predict transits --gs glasgow
curl -s 'https://celestrak.org/NORAD/elements/gp.php?CATNR=60989' | sgp4-predict transits --gs glasgow
```

**Typed in** — with no `--tle-file` and no pipe, the tool waits on stdin and prints a hint. Paste the TLE and press Ctrl-D (Ctrl-Z then Enter on Windows):

```
Reading TLE from stdin (optional name line + line 1 + line 2); Ctrl-D when done:
```

## Ground stations

Subcommands that need a ground location (`transits`, `observations`) take it as `--gs <id>`, naming a ground station defined in the config file. It is required for those subcommands.

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow --min-elevation 10
sgp4-predict observations --tle-file sentinel.tle --gs svalbard --config ./stations.yaml
```

A missing or unknown id lists what is available:

```
Error: unknown ground station 'glasgo'; known ids: glasgow, svalbard
```

### Config file

```yaml
groundstations:
  glasgow:
    location:
      latitude: 55.86
      longitude: -4.25
      altitude: 40
  svalbard:
    location:
      latitude: 78.23
      longitude: 15.39
```

| Field                  | Required | Description                              |
|------------------------|----------|------------------------------------------|
| `location.latitude`    | yes      | degrees, `[-90, 90]`                     |
| `location.longitude`   | yes      | degrees, `[-180, 180]`                   |
| `location.altitude`    | no       | metres above the ellipsoid (default: 0)  |

Unrecognised fields are rejected, so typos surface as errors rather than being silently ignored.

### Config file location

`--config <path>` selects a config file explicitly. Otherwise the tool looks in `.sgp4-predict/config.yaml` under your home directory:

| Platform      | Default path                                 |
|---------------|----------------------------------------------|
| Linux / macOS | `~/.sgp4-predict/config.yaml`                |
| Windows       | `%USERPROFILE%\.sgp4-predict\config.yaml`    |

A missing file at the default path is not an error — it just means no ground stations are defined. A `--config` path that does not exist *is* an error.

## Time range

All subcommands share `--start` and `--duration`:

```sh
--start "2026-03-25T10:00:00Z"   # ISO 8601 or loose RFC 3339 (always UTC)
--duration 3d                    # humantime format: 3d, 1h30m, 90s, etc. (default: 1d)
```

If `--start` is omitted, the current UTC time is used.

## Subcommands

### `transits` — visible passes

Finds passes above a minimum elevation. Outputs AoS, LoS, azimuth at each horizon crossing, Time of Closest Approach (TCA), and duration.

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow --min-elevation 10
```

```
aos                      los                      aos_az [deg] los_az [deg] tca_time                  tca_el [deg]   duration
-----------------------------------------------------------------------------------------------------------------------------
2026-03-25T10:14:23Z     2026-03-25T10:25:11Z           342.17        21.44 2026-03-25T10:19:47Z            72.31     10m 48s
```

### `observations` — point observations at regular intervals

Outputs azimuth, elevation, range, and range rate at each step.

```sh
sgp4-predict observations --tle-file sentinel.tle --gs glasgow --step 30s
```

```
datetime                   az [deg] el [deg]  range [km] range_rate [km/s]
--------------------------------------------------------------------------
2026-03-25T10:00:00Z         123.45    32.10     1234.56             -2.34
```

### `state-vectors` — propagated state vectors

Outputs position and velocity at each step. Use `--frame ecef` to output in ECEF instead of TEME.

```sh
sgp4-predict state-vectors --tle-file sentinel.tle --step 60s --frame teme
```

```
datetime                       x [km]       y [km]       z [km]     vx [km/s]   vy [km/s]   vz [km/s]
-----------------------------------------------------------------------------------------------------
2026-03-25T10:00:00Z       -1234.567     5678.901    -3456.789      -1.234567    6.789012    2.345678
```

### `apsides` — apogee and perigee events

```sh
sgp4-predict apsides --tle-file sentinel.tle --duration 1d
```

```
time                        event      altitude [km]
----------------------------------------------------
2026-03-25T10:34:12Z           Apogee        786.123
2026-03-25T11:22:45Z          Perigee        781.456
```

### `illumination` — sunlit and eclipse windows

```sh
sgp4-predict illumination --tle-file sentinel.tle --duration 1d
```

```
start                    end                          state   duration
----------------------------------------------------------------------
2026-03-25T10:00:00Z     2026-03-25T10:34:12Z        Sunlit    34m 12s
2026-03-25T10:34:12Z     2026-03-25T11:01:45Z       Eclipse    27m 33s
```

## Output options

| Flag                         | Description                                                                  |
|------------------------------|------------------------------------------------------------------------------|
| `-o <path>` / `--out <path>` | Write output to a file instead of stdout                                     |
| `--output-args`              | Prepend all input arguments as `# key: value` comment lines to the output    |

`--output-args` is useful for self-documenting output files. For the observer-based subcommands it records both the id and the coordinates it resolved to, as `# ground-station: <id>` and `# observer: lat,lon,alt`.

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow --output-args -o passes.txt
```
