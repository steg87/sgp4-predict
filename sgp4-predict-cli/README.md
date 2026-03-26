# sgp4-predict-cli

[![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict-cli)](../LICENSE-MIT)

A command-line tool for SGP4 satellite pass prediction. Provides tabular output for transits, observations, state vectors, apsides, and illumination windows.

## Installation

```sh
cargo install sgp4-predict-cli
```

## TLE input

All subcommands accept a TLE via `--tle-file`. If omitted, the tool prompts interactively on stdin.

Fresh TLEs can be obtained from [CelesTrak](https://celestrak.org). SGP4 accuracy degrades with TLE age — for LEO satellites, treat TLEs older than 3–7 days with caution. The tool will warn if the TLE epoch is stale.

**File** (2-line or 3-line format):

```
SENTINEL-2C
1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740
```

**Interactive prompt:**

```
Satellite name (leave blank to skip): SENTINEL-2C
TLE line 1: 1 60989U ...
TLE line 2: 2 60989 ...
```

## Observer

Subcommands that require a ground location accept `--observer "lat_deg,lon_deg,alt_m"`. If omitted, the tool prompts interactively.

```sh
--observer "55.86,-4.25,40"   # Glasgow, 40 m altitude
```

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
sgp4-predict transits --tle-file sentinel.tle --observer "55.86,-4.25,40" --min-elevation 10
```

```
aos                      los                      aos_az [deg] los_az [deg] tca_time                  tca_el [deg]   duration
-----------------------------------------------------------------------------------------------------------------------------
2026-03-25T10:14:23Z     2026-03-25T10:25:11Z           342.17        21.44 2026-03-25T10:19:47Z            72.31     10m 48s
```

### `observations` — point observations at regular intervals

Outputs azimuth, elevation, range, and range rate at each step.

```sh
sgp4-predict observations --tle-file sentinel.tle --observer "55.86,-4.25,40" --step 30s
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

`--output-args` is useful for self-documenting output files:

```sh
sgp4-predict transits --tle-file sentinel.tle --observer "55.86,-4.25,40" --output-args -o passes.txt
```
