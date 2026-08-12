# sgp4-predict-cli

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli)
![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict-cli)

A command-line tool for SGP4 satellite pass prediction: transits, observations, state vectors,
apsides, illumination windows, ground tracks, and area-of-interest overpasses, as text, JSON, or
CSV.

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
an example station on first run; a `--config` path that does not exist is an error.

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
| -------------------- | -------- | --------------------------------------- |
| `location.latitude`  | yes      | degrees, `[-90, 90]`                    |
| `location.longitude` | yes      | degrees, `[-180, 180]`                  |
| `location.altitude`  | no       | metres above the ellipsoid (default: 0) |

Unrecognised fields are rejected, so typos surface as errors.

### Managing stations

Edit the file by hand, or use `sgp4-predict gs add|list|remove`. All three operate on `--config`, or
the default path when it is omitted.

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

`aoi-windows` needs a region on the ground, given as `--aoi <id>` naming an entry in the same
config file. AOIs live under `aois:` alongside `groundstations:`, and each is a flat map of named
fields tagged with its `shape`:

```yaml
aois:
  scotland:
    shape: box
    south: 54.0
    north: 60.0
    west: -8.0
    east: -1.0
  north-sea:
    shape: circle
    latitude: 56.0
    longitude: 2.0
    radius: 2.7
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

| `shape`   | Fields                                                            |
| --------- | ----------------------------------------------------------------- |
| `box`     | `south`, `north`, `west`, `east` — the box's bounds               |
| `circle`  | `latitude`, `longitude` (centre), `radius`                        |
| `polygon` | `vertices`, a list of at least three `latitude`/`longitude` pairs |

**Everything is in degrees**, and every extent is degrees of arc — about 111.2 km per degree.

A box's north and south edges follow their parallels exactly. It runs **eastward** from `west`, so
an `east` at a smaller longitude wraps across the antimeridian rather than being an error:

```yaml
pacific: # 160°E round to 160°W, across the dateline
  shape: box
  south: -20.0
  north: 20.0
  west: 160.0
  east: -160.0
```

Polygon edges, by contrast, are great-circle arcs, so they are not lines of constant latitude — over
a wide span they bow toward the nearer pole. Use `box` when the region really is a latitude/longitude
box.

A circle's `radius` is in `(0, 90)` degrees of arc.

### Managing AOIs

Edit the file by hand, or use `sgp4-predict aoi add|list|remove`, which mirrors `gs`.

`aoi add` prompts field by field, like `gs add`. The underlined initial is accepted on its own, so
the shape can be picked with a single key:

```
$ sgp4-predict aoi add
AOI id: scotland
Shape (box, circle, polygon): b
South latitude (degrees): 54
North latitude (degrees): 60
West longitude (degrees): -8
East longitude (degrees): -1
added aoi 'scotland' (54..60, 7 eastward from -8) to /home/you/.sgp4-predict/config.yaml
```

A polygon has no fixed number of fields, so its vertices are read one per line until a blank line:

```
$ sgp4-predict aoi add corridor
AOI id: corridor
Shape (box, circle, polygon): p
Vertices, one per line as `lat,lon`. Blank line when done.
Vertex 1 lat,lon (degrees): 54,-8
Vertex 2 lat,lon (degrees): 54,-1
Vertex 3 lat,lon (degrees): 60,-1
Vertex 4 lat,lon (degrees):
added aoi 'corridor' (3 vertices) to /home/you/.sgp4-predict/config.yaml
```

Any prompt that is answered with a bad value is asked again rather than aborting — a line that does
not parse, a blank line before the third vertex, a latitude past a pole, or a bound that contradicts
one already given, such as a `north` below the `south`.

The id and the shape may be given as arguments, in which case they are echoed as though they had been
typed and only the coordinates are asked for:

```sh
sgp4-predict aoi add scotland
sgp4-predict aoi add scotland --shape box
```

**Coordinates are never taken as arguments.** As with `gs add`, an AOI is either entered at the
prompts or written into the config file by hand.

`aoi list` honours `--format` and shows each AOI by its config field names:

```
id               shape    definition
------------------------------------
cape-town        circle   latitude=-33.9 longitude=18.4 radius=2.25
corridor         polygon  (54, -8) (54, -1) (60, -1)
north-sea        circle   latitude=56 longitude=2 radius=2.7
scotland         box      south=54 north=60 west=-8 east=-1
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

The windows in which the AOI named by `--aoi <id>` is within the payload's reach, with the
sub-satellite point at each boundary crossing. See [Areas of interest](#areas-of-interest).

```sh
sgp4-predict aoi-windows --tle-file sentinel.tle --aoi europe --duration 12h
```

`--max-off-nadir <deg>` is the half-angle of the satellite's field of regard — the largest nadir
angle the payload can be slewed to. It defaults to 0, which detects the ground track itself crossing
into the area. `--coverage full` requires every part of the area to be in reach at once, rather than
any part of it:

```sh
sgp4-predict aoi-windows --aoi europe --max-off-nadir 30 --coverage full
```

```
entry                    exit                     entry_lat [deg] entry_lon [deg] exit_lat [deg] exit_lon [deg]   duration
--------------------------------------------------------------------------------------------------------------------------
2025-12-22T19:40:33Z     2025-12-22T19:43:28Z             55.0985         30.0000        65.0000        22.9317     2m 54s
2025-12-22T21:16:56Z     2025-12-22T21:24:10Z             40.0000         11.0870        65.0000        -2.2431     7m 14s
```

## Output

`--format` selects `text`, `json` or `csv`; `-o` / `--out` writes to a file instead of stdout. Every
subcommand supports all three formats with the same columns. `text` is fixed-width with a header;
`json` is newline-delimited, one object per row, so it streams into `jq`; `csv` is RFC 4180 with a
header row and the same field names as JSON.

```sh
sgp4-predict transits --gs glasgow --format json | jq 'select(.tca_el_deg > 30)'
```

```json
{
  "aos": "2026-03-25T11:36:24Z",
  "los": "2026-03-25T11:51:28Z",
  "aos_az_deg": 15.43,
  "los_az_deg": -158.39,
  "tca_time": "2026-03-25T11:43:58Z",
  "tca_el_deg": 82.95,
  "duration": "15m 4s"
}
```

`text` and `csv` write the header even when there are no rows; `json` writes nothing.

`--output-args` records how an output file was produced — the TLE, the interval, the coordinates
`--gs` resolved to, and every detection knob, whether or not it was passed:

```
# command: transits
# satellite: SENTINEL-2C
# tle-line1: 1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
# tle-line2: 2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740
# start: 2026-03-25T10:00:00Z
# duration: 4h
# ground-station: glasgow
# observer: 55.86,-4.25,40
# min-elevation: 0
# min-step: 10s
# max-step: 10m
# walk-step: 30s
# max-transit-duration: 1h
# skip-leading-partial: true
# clamp-to-interval: false
# tca-scan-step: 10s
# time-tolerance: 0.001
# max-iter: 100
```

Every line names the flag that sets it, so a recorded run can be replayed by pasting its own header
back onto the command line. It is rejected with `--format json` or `--format csv`, where `#` lines
would make the output unparseable.

## Logging

Warnings and prompts go to stderr, so they never mix into piped output. `-q` silences everything but
errors, `-v` / `-vv` / `-vvv` raise the level to info / debug / trace, and `RUST_LOG` overrides both
if set.

## Completions and man page

Generated from the live argument definitions, so they cannot drift from the actual flags.

```sh
sgp4-predict completions bash > /etc/bash_completion.d/sgp4-predict
sgp4-predict completions zsh  > ~/.zfunc/_sgp4-predict
sgp4-predict man > /usr/local/share/man/man1/sgp4-predict.1
```

## Exit codes

| Code | Meaning                                                     |
| ---- | ----------------------------------------------------------- |
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
