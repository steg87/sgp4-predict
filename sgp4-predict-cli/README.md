# sgp4-predict-cli

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli)
![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict-cli)

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

## Areas of interest

`aoi-windows` needs a region on the ground, given as `--area <id>` naming an entry in the same
config file. Areas live under `areas:` alongside `groundstations:`, and each is a flat map of named
fields tagged with its `shape`:

```yaml
areas:
  scotland:
    shape: box
    latitude: 57.0
    longitude: -4.5
    width: 7.0
    height: 6.0
  north-sea:
    shape: ellipse
    latitude: 56.0
    longitude: 2.0
    semi_major: 2.7
    semi_minor: 1.1
    bearing: 45.0
  cape-town:
    shape: circle
    latitude: -33.9
    longitude: 18.4
    radius: 2.25
  corridor:
    shape: polygon
    vertices:
      - { latitude: 54.0, longitude: -8.0 }
      - { latitude: 54.0, longitude: -1.0 }
      - { latitude: 60.0, longitude: -1.0 }
```

| `shape`   | Fields                                                          |
|-----------|-----------------------------------------------------------------|
| `box`     | `latitude`, `longitude` (centre), `width`, `height`             |
| `ellipse` | `latitude`, `longitude` (centre), `semi_major`, `semi_minor`, `bearing` (default 0) |
| `circle`  | `latitude`, `longitude` (centre), `radius`                       |
| `polygon` | `vertices`, a list of at least three `latitude`/`longitude` pairs |

**Every extent is in degrees of arc**, about 111.2 km per degree. A box's `width` is an extent in
*longitude* and its `height` an extent in latitude, so its ground width shrinks with the cosine of
its latitude; its north and south edges follow their parallels exactly. Polygon edges are
great-circle arcs, so they are not lines of constant latitude — use `box` when the region really is a
latitude/longitude box.

An ellipse's semi-axes are **not** latitude and longitude extents. `semi_major` is half the length of
the *longer* axis and `semi_minor` half the *shorter* — they must satisfy
`0 < semi_minor <= semi_major < 90` — and `bearing` is what points them, turning the major axis
clockwise from north. So `bearing: 0` (the default) runs the long axis north–south, and `bearing: 90`
runs it east–west:

```yaml
  tall:                 # 10 degrees north-south by 2 east-west
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_major: 10.0
    semi_minor: 2.0
  wide:                 # the same ellipse turned a quarter turn
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_major: 10.0
    semi_minor: 2.0
    bearing: 90.0
```

### Managing areas

Edit the file by hand, or use `sgp4-predict aoi`.

| Command                           | Description                          |
|-----------------------------------|--------------------------------------|
| `aoi add <id> <shape>`            | Add an area                          |
| `aoi list` (`aoi ls`)             | List the configured areas            |
| `aoi remove <id>` (`aoi rm <id>`) | Remove an area, after confirmation   |

`aoi add` prompts field by field, like `gs add`. The underlined initial is accepted on its own, so
the shape can be picked with a single key:

```
$ sgp4-predict aoi add
Area id: scotland
Shape (box, ellipse, circle, polygon): b
Centre latitude (degrees): 57
Centre longitude (degrees): -4.5
Width (degrees of longitude): 7
Height (degrees of latitude): 6
added area 'scotland' (54..60, 7 eastward from -8) to /home/you/.sgp4-predict/config.yaml
```

A polygon has no fixed number of fields, so its vertices are read one per line until a blank line:

```
$ sgp4-predict aoi add corridor
Shape (box, ellipse, circle, polygon): p
Vertices, one per line as `lat,lon`. Blank line when done.
Vertex 1 lat,lon (degrees): 54,-8
Vertex 2 lat,lon (degrees): 54,-1
Vertex 3 lat,lon (degrees): 60,-1
Vertex 4 lat,lon (degrees):
added area 'corridor' (3 vertices) to /home/you/.sgp4-predict/config.yaml
```

A line that does not parse is reported and asked for again, so a typo costs one line rather than
everything entered before it. The same is true of a blank line before the third vertex.

Every field can also be given up front, which is how areas are added non-interactively. The id is
positional and the shape is exactly one of four mutually exclusive flags; whatever is supplied is not
prompted for:

```sh
sgp4-predict aoi add scotland  --box 57,-4.5,7,6          # centre, then width and height
sgp4-predict aoi add north-sea --ellipse 56,2,2.7,1.1,45  # centre, axes, optional bearing
sgp4-predict aoi add cape-town --circle -33.9,18.4,2.25   # centre and radius
sgp4-predict aoi add corridor  --poly "(54,-8),(54,-1),(60,-1)"
```

Parentheses are shell metacharacters, so quote a `--poly` value. Adding over an existing id needs
`-f` / `--force`. Geometry the library rejects — an ellipse whose semi-minor axis exceeds its
semi-major, a box running past a pole — is refused before anything is written.

`aoi list` honours `--format` and shows each area by its config field names:

```
id               shape    definition
------------------------------------
cape-town        circle   latitude=-33.9 longitude=18.4 radius=2.25
corridor         polygon  (54, -8) (54, -1) (60, -1)
north-sea        ellipse  latitude=56 longitude=2 semi_major=2.7 semi_minor=1.1 bearing=45
scotland         box      latitude=57 longitude=-4.5 width=7 height=6
```

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

### `ground-track` — the sub-satellite point

The geodetic point directly beneath the satellite, sampled at `--step` (default: `60s`).

```sh
sgp4-predict ground-track --tle-file sentinel.tle --duration 20m --step 5m
```

```
datetime                  lat [deg]   lon [deg]  altitude [km]
--------------------------------------------------------------
2025-12-22T12:00:00Z       -29.3239    -27.2842        800.274
2025-12-22T12:05:00Z       -46.8701    -32.9106        807.646
2025-12-22T12:10:00Z       -64.0128    -42.8591        814.142
2025-12-22T12:15:00Z       -79.0528    -76.9861        817.736
```

### `aoi-windows` — overpasses of an area of interest

The windows in which the ground track lies inside the area named by `--area <id>`, with the
sub-satellite point at each boundary crossing. See [Areas of interest](#areas-of-interest).

```sh
sgp4-predict aoi-windows --tle-file sentinel.tle --area europe --duration 12h
```

```
entry                    exit                     entry_lat [deg] entry_lon [deg] exit_lat [deg] exit_lon [deg]   duration
--------------------------------------------------------------------------------------------------------------------------
2025-12-22T19:40:33Z     2025-12-22T19:43:28Z             55.0985         30.0000        65.0000        22.9317     2m 54s
2025-12-22T21:16:56Z     2025-12-22T21:24:10Z             40.0000         11.0870        65.0000        -2.2431     7m 14s
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

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/steg87/sgp4-predict/blob/main/LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/steg87/sgp4-predict/blob/main/LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
