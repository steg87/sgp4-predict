# sgp4-predict-cli

[![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict-cli)](../LICENSE-MIT)

A command-line tool for SGP4 satellite pass prediction: transits, observations, state vectors,
apsides, and illumination windows, as text, JSON, or CSV.

```sh
cargo install sgp4-predict-cli
```

## Getting started

Add the ground station you observe from, then predict against it:

```sh
sgp4-predict gs add                 # prompts for id, latitude, longitude, altitude
curl -s 'https://celestrak.org/NORAD/elements/gp.php?CATNR=60989' \
  | sgp4-predict transits --gs my-station --min-elevation 10
```

```
aos                      los                      aos_az [deg] los_az [deg] tca_time                 tca_el [deg]   duration
----------------------------------------------------------------------------------------------------------------------------
2026-03-25T11:38:44Z     2026-03-25T11:49:11Z            15.22      -158.01 2026-03-25T11:43:58Z            82.95    10m 27s
2026-03-25T13:19:00Z     2026-03-25T13:26:32Z            -3.83       -96.76 2026-03-25T13:22:47Z            20.65     7m 32s
```

Azimuths are degrees from north, measured clockwise, in the range `(-180, 180]` — so a southwesterly
bearing reads as `-158.01`, not `201.99`.

## TLE input

Every prediction subcommand reads a TLE from `--tle-file <path>`, or from stdin if that is omitted.
Both accept the same 2- or 3-line text; with three lines the first is the satellite name, with two
the name comes from the NORAD id (`NORAD-60989`). Blank lines and surrounding whitespace are
ignored.

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow    # from a file
cat sentinel.tle | sgp4-predict transits --gs glasgow         # piped
sgp4-predict transits --gs glasgow                            # typed, Ctrl-D to finish
```

Fresh TLEs are available from [CelesTrak](https://celestrak.org). SGP4 accuracy degrades with
element age — treat LEO TLEs older than 3–7 days with caution. The tool warns on stderr when the
epoch is stale.

## Ground stations

`transits` and `observations` need a location, given as `--gs <id>` naming an entry in the config
file. A missing or unknown id lists what is available:

```
Error: unknown ground station 'glasgo'; known ids: glasgow, svalbard
```

### The config file

`--config <path>` selects a file; otherwise `~/.sgp4-predict/config.yaml` is used
(`%USERPROFILE%\.sgp4-predict\config.yaml` on Windows). The default path is created and seeded with
an example station on first run. A `--config` path that does not exist is an error, so a typo
cannot quietly succeed against an empty config.

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

| Field                | Required | Description                             |
|----------------------|----------|-----------------------------------------|
| `location.latitude`  | yes      | degrees, `[-90, 90]`                    |
| `location.longitude` | yes      | degrees, `[-180, 180]`                  |
| `location.altitude`  | no       | metres above the ellipsoid (default: 0) |

Unrecognised fields are rejected, so typos surface as errors.

### Managing stations

Edit the file by hand, or use `sgp4-predict gs`. All three operate on `--config`, or the default
path when it is omitted.

| Command                         | Description                             |
|---------------------------------|-----------------------------------------|
| `gs add`                        | Add a station, prompting for each field |
| `gs list` (`gs ls`)             | List the configured stations            |
| `gs remove <id>` (`gs rm <id>`) | Remove a station, after confirmation    |

```
$ sgp4-predict gs add
Ground station id: svalbard
Latitude (degrees): 78.23
Longitude (degrees): 15.39
Altitude (metres) [0]:
added ground station 'svalbard' to /home/you/.sgp4-predict/config.yaml
```

`gs list` honours `--format`, so stations can be scripted against:

```sh
sgp4-predict gs list --format json | jq -r 'select(.latitude > 70) | .id'
```

`gs remove` prints the station and asks before deleting; `-f` / `--force` skips the prompt. Anything
other than `y`/`yes` — including end-of-input — leaves the config untouched.

Note that `gs add` and `gs remove` re-serialise the file, so **YAML comments are not preserved**. A
config that fails to parse is never overwritten.

## Subcommands

All prediction subcommands share `--start` (ISO 8601 or loose RFC 3339, UTC, default: now) and
`--duration` (humantime: `3d`, `1h30m`, `90s`; default: `1d`).

### `transits` — visible passes

AoS, LoS, azimuth at each horizon crossing, time of closest approach (TCA), and duration.
`--min-elevation <deg>` sets the threshold (default: 0).

```sh
sgp4-predict transits --tle-file sentinel.tle --gs glasgow --min-elevation 10
```

### `observations` — azimuth, elevation, range, and range rate

Sampled every `--step` (default: `60s`).

```sh
sgp4-predict observations --tle-file sentinel.tle --gs glasgow --step 30s
```

```
datetime                 az [deg] el [deg] range [km] range_rate [km/…
----------------------------------------------------------------------
2026-03-25T10:00:00Z        40.22    10.03    2366.31            -4.64
2026-03-25T10:00:30Z        44.43    11.90    2231.97            -4.30
```

### `state-vectors` — propagated position and velocity

Sampled every `--step` (default: `60s`), in `--frame teme` (default) or `--frame ecef`.

```sh
sgp4-predict state-vectors --tle-file sentinel.tle --step 60s --frame ecef
```

```
datetime                       x [km]       y [km]       z [km]    vx [km/s]    vy [km/s]    vz [km/s]
------------------------------------------------------------------------------------------------------
2026-03-25T10:00:00Z         2451.498     1326.624     6595.337     7.040120     0.331418    -2.679071
2026-03-25T10:01:00Z         2868.960     1342.094     6421.831     6.870563     0.184333    -3.102604
```

### `apsides` — apogee and perigee events

```sh
sgp4-predict apsides --tle-file sentinel.tle --duration 1d
```

```
time                          event  altitude [km]
--------------------------------------------------
2026-03-25T10:51:16Z         Apogee        796.366
2026-03-25T11:35:46Z        Perigee        781.430
```

### `illumination` — sunlit and eclipse windows

```sh
sgp4-predict illumination --tle-file sentinel.tle --duration 1d
```

```
start                    end                           state   duration
-----------------------------------------------------------------------
2026-03-25T10:00:00Z     2026-03-25T10:51:08Z         Sunlit     51m 8s
2026-03-25T10:51:08Z     2026-03-25T11:24:48Z        Eclipse    33m 40s
```

## Output

| Flag                          | Description                                                       |
|-------------------------------|-------------------------------------------------------------------|
| `--format <text\|json\|csv>`  | Output format (default: `text`)                                   |
| `-o <path>` / `--out <path>`  | Write to a file instead of stdout                                 |
| `--output-args`               | Prepend the resolved inputs as `# key: value` lines (`text` only) |

Every subcommand supports all three formats with the same columns. `text` is fixed-width with a
header; `json` is newline-delimited, one object per row, so it streams into `jq`; `csv` is RFC 4180
with a header row and the same field names as JSON.

```sh
sgp4-predict transits --gs glasgow --format json | jq 'select(.tca_el_deg > 30)'
```

```json
{"aos":"2026-03-25T11:36:24Z","los":"2026-03-25T11:51:28Z","aos_az_deg":15.43,"los_az_deg":-158.39,"tca_time":"2026-03-25T11:43:58Z","tca_el_deg":82.95,"duration":"15m 4s"}
```

`text` and `csv` write the header even when there are no rows; `json` writes nothing.

`--output-args` records how an output file was produced — the TLE source, the config file, and the
coordinates `--gs` resolved to:

```
# command: transits
# satellite: SENTINEL-2C
# tle-line1: 1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
# tle-line2: 2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740
# tle-source: sentinel.tle
# start: 2026-03-25T10:00:00Z
# duration: 4h
# config: /home/you/.sgp4-predict/config.yaml
# ground-station: glasgow
# observer: 55.86,-4.25,40
# min-elevation: 0
# format: text
```

It is rejected with `--format json` or `--format csv`, since `#` lines would make that output
unparseable.

## Logging

Warnings and prompts go to stderr, so they never mix into piped output.

| Flag                  | Effect               |
|-----------------------|----------------------|
| `-q` / `--quiet`      | Errors only          |
| `-v` / `-vv` / `-vvv` | info / debug / trace |

`RUST_LOG` overrides both if set.

## Completions and man page

Generated from the live argument definitions, so they cannot drift from the actual flags.

```sh
sgp4-predict completions bash > /etc/bash_completion.d/sgp4-predict
sgp4-predict completions zsh  > ~/.zfunc/_sgp4-predict
sgp4-predict man > /usr/local/share/man/man1/sgp4-predict.1
```

## Exit codes

| Code | Meaning                                                     |
|------|-------------------------------------------------------------|
| 0    | Success                                                     |
| 1    | Runtime error (bad TLE, unreadable config, unknown station) |
| 2    | Invalid command-line usage                                  |
| 141  | Output pipe closed early, e.g. `… \| head` (128 + SIGPIPE)  |
