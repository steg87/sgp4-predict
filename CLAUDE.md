# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo test --all-targets --all-features  # all tests — note: this skips doctests
cargo test --doc --all-features          # doctests, including the library README
make lint                                # cargo fmt + clippy (matches CI and the pre-commit hook)
make test                                # full suite (matches CI and the pre-push hook)
```

**Always run `make lint` and `make test` after making changes.** CI enforces both. The remaining `make` targets are self-documenting in the `Makefile`.

The workspace has three crates: `sgp4-predict/` (the Rust library), `sgp4-predict-py/` (the Python bindings) and `sgp4-predict-cli/` (the `sgp4-predict` binary). Each carries its own `CLAUDE.md`, loaded when you work under it — **area-of-interest geometry (`aoi.rs`) is documented in `sgp4-predict/CLAUDE.md`, and its constraints are load-bearing; read it before touching `Area`, `Polygon`, `Rectangle` or `Ellipse`.**

## Architecture

A Rust library wrapping the `sgp4` crate to provide higher-level prediction and observation iterators for satellite passes.

### Entry point: `Predictor`

`Predictor` (in `lib.rs`) is constructed from any `TleRecord` (`Predictor::from_tle`) or from `Elements` (OMM, `Predictor::new`).

`transits_iter`, `apsis_iter`, `illumination_iter`, `detect_transit` and `max_elevation` each have a `_with_opts` sibling taking an `XxxOpts`; the three iterator ones also take a trailing `refinement: Refinement` (opts before refinement), while the two one-shot methods keep reading `self.refinement` implicitly. Each `XxxOpts`'s `Default` reproduces the entry point's prior hardcoded behaviour, and step-like fields are floored to 1 second (`MIN_POSITIVE_STEP` in each module) — a zero or negative step never advances the scan and would hang the iterator.

Refinement is threaded into the underlying `WindowIter`/`EventIter` builder at construction (`.refinement(refinement)`), not mutated afterwards: there is deliberately **no** post-construction `with_refinement` on these iterators. `Predictor::with_refinement` is different — it configures the `Predictor` before any iterator exists, and `Predictor::refinement()` reads it back.

### Generic detection (`detect.rs`, opt-in `generics` feature)

`detect.rs` (`EventIter`, `WindowIter`, `Detector`, `StepStrategy`, ...) powers `ApsisIter`, `TransitIter` and `IlluminationIter` internally, so the module always compiles — but its crate-root re-exports are gated behind the off-by-default `generics` feature to keep the everyday API surface small. `DetectError` stays exported unconditionally because `TransitIter` can surface it (`Error::Detect(WindowTooLong)`). `tests/detect.rs` is gated with `#![cfg(feature = "generics")]`; `make test` and `make lint` use `--all-features` so the gated code stays covered.

### Type-safe coordinate frames

`frames.rs` uses phantom marker structs (`Teme`, `Ecef`, `Enu`) to make frame tracking a compile-time guarantee; `StateVector<F>`, `Position<F>` and `Velocity<F>` in `vectors.rs` are generic over frame, with conversions implemented on the concrete instantiations.

**All coordinates are in SI units (meters, m/s).** The `sgp4` crate outputs km/km·s⁻¹; conversion happens in `predict.rs` in the `From<sgp4::Prediction>` impl.

**Angles are type-safe** (`angle.rs`): `Degrees(f64)` and `Radians(f64)` tag a float with its unit so the two can't be mixed at a function boundary. There is deliberately no `From<f64>` for either — construction is always explicit. `Observer::latitude()`/`longitude()` take `Degrees`; `Observation::azimuth`/`elevation` are `Radians`; `min_elevation` parameters take `impl Into<Radians>` so either unit passes directly without a round-trip. Internal-only angle math (GMST, elevation rate, sun position) stays plain `f64` — it never crosses the public API, so typing it would be ceremony without payoff.

### Apsis detection (`apsides.rs`)

`ApsisIter` monitors the sign of the radial velocity `r · v` in TEME at a fixed step (60 s by default, see `ApsisIterOpts`). A sign change brackets an event: positive→negative is apogee, negative→positive is perigee. Brent's method refines the crossing time — no derivative needed, since the bracket is already known.

### Transit detection (`transits.rs`)

`TransitIter` steps adaptively — large steps when descending or far from `min_elevation`, smaller when approaching. Step bounds, the boundary-walk step and the max transit duration come from `TransitIterOpts`. On an Outside→Inside transition it refines the crossing time via `roots.rs`: Newton-Raphson first (elevation rate as the derivative), falling back to Brent's method (bracketed, guaranteed) if that fails.

### `IntervalRange` and `TimeWindow` traits (`time.rs`)

Both `Range<DateTime<Utc>>` and `Transit` implement `IntervalRange`, so a `Transit` can be passed directly as an interval to `prediction_iter` or `observation_iter` to iterate over one pass.

The two traits are deliberately separate. `IntervalRange` only _reads_ an interval, which is why every iterator takes `impl IntervalRange` — a caller's own type can span time without being reconstructible. `TimeWindow: IntervalRange + Sized` adds the one method that can't be derived from reading, `with_bounds(start, end) -> Self`, and gets `clamp` for free; it is implemented by the concrete detection results (`Transit`, `AoiWindow`, `Illumination`, `detect::Window`) only — `Range<DateTime<Utc>>` has no payload to preserve, so its `clamp` would just duplicate `IntervalRange::intersection`. Do not merge `with_bounds` into `IntervalRange` — that would force every interval-shaped type to be constructible and break the `impl IntervalRange` parameters.

`with_bounds` takes `&self` and rebuilds with `..*self` rather than mutating, so payload fields (`Illumination::state`, `Window::positive`) survive clamping. New window types belong here rather than growing another inherent `clamp`.

`DateTimeIter` substitutes 1 s for a **non-positive** step only — a zero step never advances `next_time` and would yield the same instant forever, which previously hung `prediction_iter`/`observation_iter`. Any positive step is used as given, including sub-second ones: this is a _sampling_ iterator, so `Duration::milliseconds(100)` is legitimate, unlike for the coarse detection scans. Do not "make it consistent" with those — flooring here silently decimates a caller's sample rate.

## Conventions

- **Code comments**: terse, and only where the code is non-obvious. Explain _why_ the code is the way it is — never how it used to be, why it changed, what was tried instead, or anything that reads as a transcript of the discussion that produced it. Describe what is there, in the present tense, as if it had always been that way.
- **User-facing docs** (READMEs, `docs/`, docstrings) are for someone picking the library up, not a record of how it got here. Same rule: no history, no rejected alternatives, no edge cases that only came up in review. Design rationale belongs in this file instead.
- **Standalone docs** (`docs/`, READMEs) have exactly two jobs: get a new user to their first working prediction, and take an existing user up to the advanced `generics` surface. They are not a feature tour and not an API reference — rustdoc and `examples/` are. **Never describe an API in a doc file unless the description is testable.** A runnable snippet is fine — it breaks when it drifts. Prose is not: no function signatures, no tables of methods, no lists of a module's exports or an enum's variants. Every one of those is a second copy of something rustdoc already generates, and it silently goes stale. Link to the item instead. Same for concepts: if a docstring or an example already covers it, point at it rather than restating it.

  **Every sentence must earn its place — if it does not, cut it.** Prefer a runnable snippet to prose describing one. No design decisions or rationale in the docs at all; that goes in this file. Edge cases go in a `//` comment at the code, or in the function's docstring when a caller could actually hit one — never in a standalone page.

- **Tests**: cover every code path, not every option. One test per branch, error variant and early return; do not add a test per field, per builder knob or per combination of them. When adding a knob, exercise it only where it changes behaviour.
- `sgp4-predict/README.md` is compiled as a doctest via the `Readme` struct in `lib.rs`, so its examples cannot drift. `cargo test --all-targets` does **not** run doctests — `make test` and `test.yml` run `cargo test --doc` as a separate step for exactly this reason.

### `#[must_use]` and `#[non_exhaustive]`

**`#[must_use]` sits on the iterator and builder _structs_, not their methods.** `Iterator`'s own
`#[must_use]` propagates only through `impl Iterator` return position, so a named struct like
`PredictionIter` warns about nothing — and every `*_iter` call is lazy, making a dropped one a
silent no-op. Putting the attribute on the type covers the constructor, every `-> Self` method
(`include_end`, and every builder setter), and any future method returning it, from one
line. `DetectIter` covers the `EventIter` and `WindowIter` aliases too. Do not "complete" this by
adding method-level attributes to those types; they are already covered and would be redundant.

Method-level `#[must_use]` is only for methods whose _type_ should not be must-use:
`TimeWindow::clamp` and `IntervalRange::intersection` (`Option` is not must-use), and
`TimeWindow::with_bounds`, `Predictor::with_refinement` and `Polygon::with_fill_rule` (the
receiving type is normally stored, not consumed), and both
`Angle::normalized`s (which read like in-place mutators).

Clippy's `must_use_candidate` also flags every pure getter — `Degrees::to_f64`, `Ellipse::foci`,
`Predictor::epoch`, and ~30 more. Those were considered and deliberately left off: they catch no
real bug and the churn is not worth it. Adding one is fine; a mass sweep changes the policy and
should be a decision, not a drive-by.

**`#[non_exhaustive]` is on the four public `Error` enums and deliberately _not_ on the `*Opts`
structs.** On a struct it forbids struct expressions from other crates entirely, _including_
functional-update syntax — so `AoiIterOpts { min_step: x, ..Default::default() }`, the documented
way to use them, would stop compiling downstream. Adding an `Opts` field stays a breaking change;
forcing a builder API on them is the worse trade. The cost on the error enums is that a downstream
`match` needs a `_` arm: `sgp4-predict-py/src/errors.rs` has one, mapping unknown variants to
`PyRuntimeError` since a new variant is likelier a runtime failure than bad input.

## Repo infrastructure

- **Git hooks**: managed by `prek` (`prek.toml`). Pre-commit runs fmt+clippy; pre-push runs test+coverage. Contributors install with `prek install`.
- **Dependencies**: `serde_yaml` (not `serde_yml`) is used for YAML parsing in dev/tests.

### Releasing

Full process in `docs/RELEASING.md`. `release-prepare.yml` (manual dispatch) bumps versions and rolls changelogs via `cargo-release` (`release.toml`), then opens a release PR; merging it triggers `release.yml`, which publishes, tags, and opens a GitHub Release per crate.

**All three crates share `major.minor`; `patch` moves independently.** A `minor`/`major` bump is workspace-wide and zeroes patch everywhere, so alignment is automatic; a `patch` bump may be scoped to one crate. Both workflows assert the invariant, as does `test.yml`.

Points that are deliberate and easy to "fix" wrongly:

- `release.toml`'s changelog pattern is anchored (`(?m)^## \[Unreleased\]$`). Unanchored, it also matches each changelog preamble's prose reference to that heading and trips `exactly = 1`.
- `release-prepare.yml` runs `cargo release version` and `cargo release replace` as separate steps rather than the all-in-one `cargo release <level>` — the all-in-one **commits**, and `create-pull-request` needs the changes left in the working tree.
- Pre-releases (`0.2.0-rc.1`) skip the changelog roll; `extract-changelog.sh` reads `[Unreleased]` for any version with a pre-release suffix, so an rc ships the pending notes and the final release rolls them.
- `release-prepare.yml` opens its PR branches under `release/…`, so `release.yml` triggers on `main` and `*.x` only. **Never add `release/**`to those triggers** — it would publish a release PR branch on push, before review. Maintenance branches are named`<major>.<minor>.x`(e.g.`1.1.x`) for the same reason.
- `cargo publish` is one invocation with multiple `-p`. Cargo stages the crates in a temp registry so the cli verifies against the to-be-published lib; this is why there is no index-propagation retry loop.
- Tag absence, not the commit, is the release signal, which is what makes a re-run resume instead of duplicating. **`0.0.0` is the "never released" sentinel** and is skipped: without it, any crate at an untagged version would publish on the next merge to `main`.

## Domain knowledge

This library operates in the LEO (Low Earth Orbit) domain. Meaningful review of functionality requires expertise in:

- **SGP4 propagation**: the underlying orbital mechanics model, its assumptions, and known limitations (e.g. accuracy degrades beyond ~7 days from TLE epoch).
- **Coordinate frames**: TEME (True Equator Mean Equinox), ECEF (Earth-Centred Earth-Fixed), ENU (East-North-Up). Mistakes in frame conversions produce silently wrong results.
- **Ground station geometry**: azimuth/elevation calculations, horizon masking, atmospheric refraction (not modelled here).
- **Apsis timing**: apogee/perigee detection via radial velocity sign change is correct for near-circular LEO orbits; behaviour near highly elliptical orbits should be verified carefully.
- **Illumination model**: a cylindrical shadow model is used — this is an approximation. It is adequate for most LEO scheduling use cases but will have error near the penumbra boundary.
- **TLE age**: SGP4 accuracy is sensitive to TLE age. `Predictor::tle_age()` exposes this; callers should warn or reject stale TLEs (typically > 3–7 days for LEO).
