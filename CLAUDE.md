# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                    # build the library
cargo check                    # fast type-check without full compile
cargo test --all-targets --all-features  # run all tests
cargo test <name>              # run a single test by name (e.g. cargo test test_brent_cubic)
cargo clippy                   # lint
make lint                      # cargo fmt + clippy (preferred — matches CI and pre-commit hook)
make test                      # full test suite (preferred — matches CI and pre-push hook)
make coverage                  # llvm-cov summary
make validation                # cross-validate against pypredict/skyfield reference data
make benchmark                 # Rust vs pypredict monte carlo benchmark
make docs                      # build cargo docs and open in a browser

cargo run --bin sgp4-predict -- <subcommand>  # run the CLI (see sgp4-predict-cli/README.md)
```

**Always run `make lint` and `make test` after making changes** to catch formatting, lint, and correctness issues before pushing. CI enforces both.

### Python bindings (`sgp4-predict-py/`)

Run these from within `sgp4-predict-py/`:

```bash
make dev    # compile the Rust extension in-place (maturin develop)
make test   # compile + run pytest
make lint   # ruff check --fix + ruff format (fixes in place, like the Rust make lint)
```

To regenerate stubs after Rust API changes (run from repo root):

```bash
PYO3_PYTHON=sgp4-predict-py/.venv/bin/python \
  cargo run --manifest-path sgp4-predict-py/Cargo.toml --bin stub_gen
```

Note: `make stubs` inside `sgp4-predict-py/` fails when `VIRTUAL_ENV` points elsewhere — use the explicit command above instead.

**Known stub-gen limitation**: pyo3-stub-gen silently drops static methods whose parameters are `&Bound<'_, PyAny>` (e.g. `Elements.from_dict`). Such methods work at runtime but will not appear in `_sgp4_predict/__init__.pyi`. If this becomes a problem, the method signature can be overridden in the hand-maintained `sgp4_predict/__init__.pyi`.

## Architecture

This is a Rust library (`sgp4-predict`) wrapping the `sgp4` crate to provide higher-level prediction and observation iterators for satellite passes. The workspace has three crates: `sgp4-predict/` (the Rust library), `sgp4-predict-py/` (the Python bindings), and `sgp4-predict-cli/` (the `sgp4-predict` binary).

### Entry point: `Predictor`

`sgp4-predict/src/lib.rs` defines `Predictor` as the main struct. It is constructed from any type implementing `TleRecord` (via `Predictor::from_tle`) or from `Elements` (OMM, via `Predictor::new`). It exposes:
- `propagate(t)` → `TemeState` — raw SGP4 propagation at a moment in time
- `observe_at(t, observer)` → `Observation` — azimuth/elevation/range/range_rate from a ground location
- `prediction_iter(interval, step)` → `PredictionIter`
- `observation_iter(observer, interval, step)` → `ObservationIter`
- `transits_iter(observer, interval, min_elevation)` → `TransitIter`
- `apsis_iter(interval)` → `ApsisIter`

`transits_iter`, `apsis_iter`, `illumination_iter`, `detect_transit`, and `max_elevation` each have a `_with_opts` sibling (`transits_iter_with_opts`, `apsis_iter_with_opts`, `illumination_iter_with_opts`, `detect_transit_with_opts`, `max_elevation_with_opts`) taking an additional `opts: TransitIterOpts` / `ApsisIterOpts` / `IlluminationIterOpts` / `MaxElevationOpts` (the iterator ones also take a trailing `refinement: Refinement` — opts before refinement; the two one-shot methods, `detect_transit`/`max_elevation`, take only `opts` and keep reading `self.refinement` implicitly). Each `XxxOpts` has a `Default` reproducing the entry point's prior hardcoded behavior (coarse-scan step, walk step where applicable, window/duration caps); step-like fields are floored to a minimum of 1 second (`MIN_POSITIVE_STEP` in each module) since a zero or negative step never advances the scan and would hang the iterator. Refinement is threaded into the underlying `WindowIter`/`EventIter` builder at construction time (`.refinement(refinement)`), not mutated after the iterator is built — there is deliberately no post-construction `with_refinement` on these iterators (unlike `Predictor::with_refinement`, which configures the `Predictor` itself before any iterator is created from it; `Predictor::refinement()` reads it back).

### Generic detection (`detect.rs`, opt-in `generics` feature)

The generic event/window iterators in `detect.rs` (`EventIter`, `WindowIter`, `Detector`, `StepStrategy`, ...) power `ApsisIter`, `TransitIter`, and `IlluminationIter` internally, so the module always compiles — but its public re-exports at the crate root are gated behind the off-by-default `generics` Cargo feature to keep the everyday API surface small. `DetectError` stays exported unconditionally because `TransitIter` can surface it (`Error::Detect(WindowTooLong)`). `tests/detect.rs` is gated with `#![cfg(feature = "generics")]`; `make test` and `make lint` use `--all-features` so the gated code stays covered.

### Type-safe coordinate frames

`frames.rs` uses phantom marker structs (`Teme`, `Ecef`, `Enu`) to make coordinate frame tracking a compile-time guarantee. `StateVector<F>`, `Position<F>`, and `Velocity<F>` in `vectors.rs` are all generic over frame. Conversion methods are implemented directly on the concrete instantiations:

- `StateVector<Teme>::to_ecef(t)` — GMST rotation (Z-axis) to ECEF
- `StateVector<Ecef>::to_enu(observer)` — geodetic to local East-North-Up
- `StateVector<Enu>::to_observation()` / `to_elevation()` — final observables

**All coordinates are in SI units (meters, m/s).** The `sgp4` crate outputs km/km·s⁻¹; conversion happens in `sgp4-predict/src/predict.rs` in the `From<sgp4::Prediction>` impl.

**Angles are type-safe in Rust** (`angle.rs`): `Degrees(f64)` and `Radians(f64)` tag a plain `f64` with its unit so the two can't be silently mixed up at a function boundary. There is deliberately no `From<f64>` for either — construction is always explicit (`Degrees(51.5)`, `Radians(1.2)`), and conversion goes through `.to_radians()`/`.to_degrees()` or the corresponding `From` impls; `.degrees()`/`.radians()` are one-hop shorthands for `.to_degrees().to_f64()`/`.to_radians().to_f64()`. Both types also have `.normalized()` (wrap into `[0, 360)` / `[0, 2π)`) and `.total_cmp()`. `Observer::latitude()`/`longitude()` (both the trait and `GroundObserver`) take `Degrees`; `Observation::azimuth`/`elevation` are `Radians`. `min_elevation` parameters (`transits_iter`, `detect_transit`, ...) take `impl Into<Radians>`, so a `Degrees` or `Radians` value can be passed directly — no pointless round-trip through the other unit. Internal-only angle math (GMST, elevation rate, sun-position angles) stays plain `f64` — it never crosses the public API, so typing it would be ceremony without payoff. This type safety is Rust-only: the Python bindings keep plain `float` with `_deg`-suffixed field/arg names, converting to/from `Degrees`/`Radians` at the FFI boundary.

**Python vs Rust naming**: in Rust, `Observer` is the *trait*; the concrete type is `GroundObserver`. In the Python bindings, the class is also named `GroundObserver`.

### Apsis detection (`apsides.rs`)

`ApsisIter` detects apogee and perigee events in the TEME frame with a fixed step (60 seconds by default; see `ApsisIterOpts`). It monitors the sign of the radial velocity `r · v` (dot product of position and velocity vectors). A sign change brackets an event:
- `r·v > 0 → < 0`: apogee (`ApsisEvent::Apogee`)
- `r·v < 0 → > 0`: perigee (`ApsisEvent::Perigee`)

Brent's method refines the crossing time (no derivative needed; bracket is already known).

### Transit detection (`transits.rs`)

`TransitIter` uses an adaptive step-size strategy: large steps when the satellite is descending or far from `min_elevation`, smaller steps when approaching. Step bounds, the boundary-walk step, and the max transit duration are configurable via `TransitIterOpts`. On detecting an Outside→Inside transition, it refines the exact crossing time using root finding (`roots.rs`):
1. Newton-Raphson (uses elevation rate as derivative, fast convergence)
2. Falls back to Brent's method (bracketed, guaranteed convergence) if Newton-Raphson fails

### `IntervalRange` trait (`time.rs`)

Both `Range<DateTime<Utc>>` and `Transit` implement `IntervalRange`, so a `Transit` can be passed directly as an interval to `prediction_iter` or `observation_iter` to iterate over a specific pass.

`DateTimeIter` floors its step at `MIN_POSITIVE_STEP` (1 s). A zero step never advances `next_time` and would yield the same instant forever — this previously hung `prediction_iter`/`observation_iter`. The detection paths already clamped; this brings the stepping iterator in line. Keep the two consistent.

### CLI (`sgp4-predict-cli/`)

The `sgp4-predict` binary. `cli.rs` holds clap declarations only; logic lives in sibling modules. Each subcommand's `Args` struct flattens `CommonArgs` (start/duration/tle-file/out/format/output-args), and the observer-taking subcommands (`observations`, `transits`) also flatten `ObserverArgs`. `--config`, `--verbose` and `--quiet` are `global = true` on the top-level `Args`, so they may appear on either side of the subcommand; `main.rs` passes the config path down to the two commands that need it. Errors are `anyhow`.

`commands::prepare()` builds the shared `Context` (interval, TLE, predictor, writer, format) that every subcommand needs — add new subcommands through it rather than repeating the sequence. Ordering inside it is deliberate: `--output-args`/format compatibility is checked first, then the TLE is loaded *before* the writer is opened, so a bad TLE leaves no empty `--out` file behind.

`output.rs` is column-driven: each `write_*` declares a `&[Column]` (header, JSON/CSV key, width, alignment) and emits `Cell::Str`/`Cell::Num` rows, which `RowWriter` renders as text, NDJSON, or CSV. Adding a format means adding a `Format` variant and a match arm, not touching the five commands. The text underline is derived from the rendered header (`"-".repeat(header.chars().count())`), so column widths can change without desyncing it — do not reintroduce hand-computed widths. `--output-args` is rejected for JSON/CSV because `#` lines would make that output unparseable.

`main.rs` returns `ExitCode`, not `anyhow::Result`: a broken pipe (`… | head`) exits 141 silently instead of printing an error, so piping is not reported as failure. Warnings go through `tracing` to **stderr** and never to stdout.

Ground locations come from the config file, not from CLI coordinates: `--gs <id>` names an entry in the `groundstations` map. There is deliberately no inline `--observer "lat,lon,alt"` flag — it was removed.

`commands/gs.rs` implements `gs add|remove|list` (aliases `rm`, `ls`) over `open_for_edit`/`save`. Its prompts and confirmations go to **stderr**, so `gs list` stays pipeable and prompts stay visible when stdout is redirected. `confirm()` treats EOF and anything other than `y`/`yes` as no, so a non-interactive caller that forgot `--force` cannot delete a station. `gs list` reuses the `output.rs` column machinery, so it honours `--format` like every other table.

Note the asymmetry with the data-input paths below: `gs add` prompts deliberately because it is interactive config management, whereas TLE and observer input prompts were removed so they could be piped. Do not "restore consistency" by removing this one.

Neither *data* input prompts line-by-line any more; both were removed in favour of non-interactive paths. `--tle-file` reads a file, and omitting it reads *all* of stdin so a TLE can be piped in. `tle.rs` funnels both through one `parse_tle(&str)`, so file and pipe accept exactly the same 2-or-3-line text — keep it that way rather than adding a parser per source. When stdin is a terminal, `read_tle_stdin` prints a Ctrl-D hint to **stderr**, not stdout, so it cannot contaminate piped output. Note that the observer-taking commands resolve `--gs` *before* calling `load_tle`, so a bad station id fails immediately instead of after the user has typed a TLE.

`config.rs` deserializes the YAML (`groundstations: {id: {location: {latitude, longitude, altitude}}}`; `altitude` defaults to 0, and every struct is `deny_unknown_fields` so typos error rather than being silently dropped). The path comes from `--config`, else `dirs::home_dir().join(".sgp4-predict").join("config.yaml")` — one expression covering `~/.sgp4-predict/config.yaml` and `%USERPROFILE%\.sgp4-predict\config.yaml`, so keep new path handling `PathBuf`-based rather than string-formatted. Creation is deliberately asymmetric between the default path and `--config`. A missing file at the *default* path is created and seeded with `TEMPLATE` — the user never named it, so it cannot be a typo. A missing `--config` path is an **error** everywhere except `gs add`: creating it would let a mistyped path succeed against a fresh empty config while the real stations sit unread, and the resulting `unknown ground station` error points at the wrong file. Do not "simplify" this into one rule; the two cases differ in whether the user typed the path.

There are two entry points. `load()` is for the prediction commands and behaves as above. `open_for_edit(path, Missing)` is for the `gs` commands and never seeds — `gs add` passes `Missing::Create` and starts from an empty config, `gs list`/`gs remove` pass `Missing::Reject`. The reject applies only to an *explicit* path; a missing default path is still just an empty station list. Both entry points propagate parse errors, so a broken config is never silently overwritten. `Config::save()` writes to a sibling `.yaml.tmp` and renames, so a failed write cannot truncate an existing config; it re-emits a fixed header because **serialising drops YAML comments**, which is the known cost of `gs add`/`gs remove` on a hand-annotated file.

`GroundStation` implements the library's `Observer` trait directly, so the CLI never constructs a `GroundObserver` — it hands `&GroundStation` straight to `observation_iter` / `transits_iter` / `observe_at`, which are all generic over `O: Observer`. This is the "implement the trait on your own type" path the library README documents; don't reintroduce a conversion. `GroundObserver` remains the library's built-in type for users who lack one of their own, and the Python bindings define a separate pyclass of the same name.

Coordinate range checks live in `Location::validate()` and run in `Config::groundstation()`, the only way to get a `&GroundStation` — deserialization itself does not validate, so a `GroundStation` obtained by any other route (e.g. indexing `groundstations` directly) is unchecked. Validation is per-lookup, not per-load, so one malformed entry does not block using the others.

`ObserverArgs` is the mixin that carries this: `validate(&Config)` enforces that `--gs` is present and names a usable station (returning the id), and `resolve(config_path)` loads the config and `remove`s the named station to return it owned — owned rather than borrowed because the `Config` is local to `resolve`. Errors list the ids the config actually defines, via `Config::ids_hint()` / `Config::groundstation()` — preserve that when touching these messages, and note that `tests/config.rs` asserts on the wording. Both commands then `.expect()` on `args.observer.gs` when writing the `--output-args` header, which is sound only because `resolve` ran first.

## Conventions

- **Code comments**: keep terse. State the non-obvious fact, not the reasoning behind it or alternatives considered.

## Repo infrastructure

- **Git hooks**: managed by `prek` (`prek.toml`). Pre-commit runs fmt+clippy; pre-push runs test+coverage. Contributors install with `prek install`.
- **CI** (`.github/workflows/`):
  - `test.yml` — runs `cargo test`, `cargo fmt --check`, `cargo clippy`, and `cargo doc` (denying rustdoc warnings). Installs `uv` in the test and docs jobs.
  - `audit.yml` — weekly `cargo audit` for security advisories.
  - `labeler.yml` — auto-labels PRs based on changed files (config in `.github/labeler.yml`).
- **Dependencies**: `serde_yaml` (not `serde_yml`) is used for YAML parsing in dev/tests.

## Domain knowledge

This library operates in the LEO (Low Earth Orbit) domain. Meaningful review of functionality requires expertise in:

- **SGP4 propagation**: the underlying orbital mechanics model, its assumptions, and known limitations (e.g. accuracy degrades beyond ~7 days from TLE epoch).
- **Coordinate frames**: TEME (True Equator Mean Equinox), ECEF (Earth-Centred Earth-Fixed), ENU (East-North-Up). Mistakes in frame conversions produce silently wrong results.
- **Ground station geometry**: azimuth/elevation calculations, horizon masking, atmospheric refraction (not modelled here).
- **Apsis timing**: apogee/perigee detection via radial velocity sign change is correct for near-circular LEO orbits; behaviour near highly elliptical orbits should be verified carefully.
- **Illumination model**: a cylindrical shadow model is used — this is an approximation. It is adequate for most LEO scheduling use cases but will have error near the penumbra boundary.
- **TLE age**: SGP4 accuracy is sensitive to TLE age. `Predictor::tle_age()` exposes this; callers should warn or reject stale TLEs (typically > 3–7 days for LEO).
