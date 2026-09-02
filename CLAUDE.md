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

The workspace has three crates: `sgp4-predict/` (the Rust library), `sgp4-predict-py/` (the Python bindings) and `sgp4-predict-cli/` (the `sgp4-predict` binary). Each carries its own `CLAUDE.md`, loaded when you work under it — **area-of-interest geometry (`aoi.rs`) is documented in `sgp4-predict/CLAUDE.md`, and its constraints are load-bearing; read it before touching `Area`, `Polygon`, `Rectangle` or `Circle`.**

## Architecture

A Rust library wrapping the `sgp4` crate to provide higher-level prediction and observation iterators for satellite passes.

### Entry point: `Predictor`

`Predictor` (in `lib.rs`) is constructed from any `TleRecord` (`Predictor::from_tle`) or from `Elements` (OMM, `Predictor::new`).

`transits_iter`, `apsis_iter`, `illumination_iter`, `detect_transit` and `max_elevation` each have a `_with_opts` sibling taking an `XxxOpts`; the three iterator ones also take a trailing `refinement: Refinement` (opts before refinement), while the two one-shot methods keep reading `self.refinement` implicitly. Each `XxxOpts`'s `Default` reproduces the entry point's prior hardcoded behaviour, and step-like fields are floored to 1 second (`MIN_POSITIVE_STEP` in each module) — a zero or negative step never advances the scan and would hang the iterator.

Refinement is threaded into the underlying `WindowIter`/`EventIter` builder at construction (`.refinement(refinement)`), not mutated afterwards: there is deliberately **no** post-construction `with_refinement` on these iterators. `Predictor::with_refinement` is different — it configures the `Predictor` before any iterator exists, and `Predictor::refinement()` reads it back.

### Iterator error handling (`fallible.rs`)

The iterators keep yielding `Result` rather than swallowing errors, because the two error classes
that reach `next()` want opposite handling. **Local**: `Roots::FailedToConverge`,
`Roots::Unbracketed`, `Detect::WindowTooLong` — one event failed to refine, and `DetectIter`
advances `current` before calling `detect_event`, so the scan is not wedged; skipping is right.
**Sticky**: `Error::Sgp4` is degenerate propagation state, deterministic in the elements and `t`, so
a decayed TLE fails at *every* sample. Blanket-skipping turns that into an empty iterator —
indistinguishable from "no passes this week", a silent no-op in scheduling code. Only the call site
knows which it can tolerate. Dropping `Result` is also a one-way door: an adapter is additive at any
time, error reporting cannot be added back.

`FallibleIter` is a blanket impl over `Iterator<Item = Result<T>>`, so all eight fallible iterators
gain it including the lifetime-parameterised ones, with no existing signature changed.

- **`Tolerate` counts _consecutive_ errors, resetting the run on any `Ok`.** This is the
  data-derived substitute for an `Error::is_transient()` classifier: an unbroken run of N failures
  is observed evidence the object is dead, so it degrades correctly for variants not yet added
  (`Error` is `#[non_exhaustive]`) and cannot mis-classify `Error::Custom`. `until_error` is
  `tolerate_errors(0)`.
- **`skip_errors`/`log_errors` reuse `OnError` through `fn(Error)` coercion**, not closures, so both
  return a nameable type and there is one skip-style struct rather than three.
- `skip_errors` duplicates `Iterator::flatten`, which also drops the `Err`s, and is kept anyway:
  `flatten` on a `Result` iterator reads like a bug. Prefer `skip_errors` at every call site —
  `tests/examples.rs` uses it throughout.
- `Tolerate` owns its terminating error and lends it back; hence the `&mut` iteration in the docs.

Deliberately absent: an `inspect_errors` that would let `log_errors` and `tolerate_errors` chain.
`Iterator::inspect` covers it in one line today.

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

The two traits are deliberately separate. `IntervalRange` only _reads_ an interval, which is why every iterator takes `impl IntervalRange` — a caller's own type can span time without being reconstructible. `TimeWindow: IntervalRange + Sized` adds the one method that can't be derived from reading, `with_bounds(start, end) -> Self`, and gets `clamp_to` for free; it is implemented by the concrete detection results (`Transit`, `AoiWindow`, `Illumination`, `detect::Window`) only — `Range<DateTime<Utc>>` has no payload to preserve, so its `clamp_to` would just duplicate `IntervalRange::intersection`. Do not merge `with_bounds` into `IntervalRange` — that would force every interval-shaped type to be constructible and break the `impl IntervalRange` parameters.

The `impl IntervalRange` parameters are by value, with a blanket `impl<T: IntervalRange + ?Sized> IntervalRange for &T` — the same shape as `TleRecord for &T` and `Area for &A`. A caller whose interval type isn't `Copy` passes `&interval` and nothing is moved or cloned; `a..b` still passes without parentheses. Do not "fix" the move by switching the parameters to `&impl IntervalRange`: that is breaking, forces `&(a..b)` at every call site, and buys nothing the blanket impl doesn't already give.

`with_bounds` takes `&self` and rebuilds with `..*self` rather than mutating, so payload fields (`Illumination::state`, `Window::positive`) survive clamping. New window types belong here rather than growing another inherent `clamp_to`.

`clamp_to` is named that way to stay clear of `Ord::clamp`. The two would collide on every implementor: `Ord::clamp` takes `self` by value and `clamp_to` takes `&self`, so method resolution reaches the by-value receiver first and `Ord::clamp` wins outright. Their arities differ, so today it is a compile error rather than a silently wrong call — do not rely on that. The window types are `DateTime` pairs and so are totally ordered; they derive `Ord` and belong in a `BTreeSet`, which is what the name buys.

`DateTimeIter` substitutes 1 s for a **non-positive** step only — a zero step never advances `next_time` and would yield the same instant forever, which previously hung `prediction_iter`/`observation_iter`. Any positive step is used as given, including sub-second ones: this is a _sampling_ iterator, so `Duration::milliseconds(100)` is legitimate, unlike for the coarse detection scans. Do not "make it consistent" with those — flooring here silently decimates a caller's sample rate.

## Conventions

- **Code comments**: terse, and only where the code is non-obvious. Explain _why_ the code is the way it is — never how it used to be, why it changed, what was tried instead, or anything that reads as a transcript of the discussion that produced it. Describe what is there, in the present tense, as if it had always been that way.
- **User-facing docs** (READMEs, `docs/`, docstrings) are for someone picking the library up, not a record of how it got here. Same rule: no history, no rejected alternatives, no edge cases that only came up in review. Design rationale belongs in this file instead.
- **`#![warn(missing_docs)]` is on the library**, and `make lint`'s `-D warnings` makes it fatal, so a new public item — including an error variant or a variant's field — fails CI until it is documented. Same mechanism as `clippy::must_use_candidate`; it is not something to remember.

- **The user-facing overview of the `generics` feature lives in `lib.rs`'s crate docs**, not in `detect.rs`. `detect` is a private module whose items are re-exported, so rustdoc renders nothing for its `//!` block — anything written there is invisible. `detect.rs` keeps only a short orientation note for maintainers. The equator-crossing doctest is gated with `cfg_attr(feature = "generics", doc = "```no_run")` so it compiles under `--all-features` and is skipped otherwise.

- **Every `Error` variant's payload type is exported**, `RootsError` included. A variant whose payload cannot be named forces the caller into a wildcard match.

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

Method-level `#[must_use]` covers two groups. The first is methods whose _type_ should not be
must-use: `TimeWindow::clamp`, `IntervalRange::intersection` and `Tolerate::error`/`into_error`
(`Option` is not must-use), and
`TimeWindow::with_bounds`, `Predictor::with_refinement` and `Polygon::with_fill_rule` (the
receiving type is normally stored, not consumed), and
`Degrees::normalized`/`Radians::normalized` (which read like in-place mutators).

The second is every pure getter, which `clippy::must_use_candidate` flags. The lint is `warn` in
`[workspace.lints.clippy]` and `make lint`'s `-D warnings` makes it fatal, so the attribute is not
something to remember to add — a new getter fails CI without it. Trait methods and `&mut self`
methods are outside the lint's reach; leave them alone unless dropping the result is a real bug.

**`#[non_exhaustive]` is on the four public `Error` enums and deliberately _not_ on the `*Opts`
structs.** On a struct it forbids struct expressions from other crates entirely, _including_
functional-update syntax — so `AoiIterOpts { min_step: x, ..Default::default() }`, the documented
way to use them, would stop compiling downstream. Adding an `Opts` field stays a breaking change;
forcing a builder API on them is the worse trade. The cost on the error enums is that a downstream
`match` needs a `_` arm: `sgp4-predict-py/src/errors.rs` has one, mapping unknown variants to
`PyRuntimeError` since a new variant is likelier a runtime failure than bad input.

### Derives

**Every error enum is `Clone + PartialEq`, the root `Error` included.** The three foreign types it
wraps (`sgp4::Error`, `sgp4::TleError`, `sgp4::ElementsError`) all derive both, so nothing forces
the root to stop at `Debug` — a caller can `assert_eq!` on a whole `Result` without matching the
variant out first, which is what makes the sub-error derives reachable at all. `Eq` is out
everywhere: `roots::Error::FailedToConverge` carries `f64`s.

**Where a generic is only held behind a shared reference, the trait impls are hand-written**
(`TransitIter`, `ObservationIter`, `AoiIter`, `ElevationAboveMin`, `GroundTrackInside`, and
`ValueFn`/`RateFn` in `detect.rs`). `OnError`'s `Debug` is hand-written for the same reason —
`on_error`'s general case is a closure, so a derive's `F: Debug` bound would be dead exactly where
the type is most used. It is `Clone` but deliberately not `Copy`: std's iterator adapters aren't,
because a `Copy` iterator gets silently copied into a `for` loop leaving the original unadvanced. A derive bounds on the type parameter itself, so
`#[derive(Clone)]` on `TransitIter<'a, O>` would emit `where O: Clone` for a field that is a
`&'a O` — making the iterator un-`Clone` for any caller-supplied `Observer` that isn't. `ValueFn`
is the sharper case: `F` is always a closure and closures are never `Debug`, so a derived `Debug`
would be dead for the type's entire intended use and would propagate up through
`WindowDetector`/`DetectIter` to make the whole `generics` surface un-`Debug`. Do not "simplify"
these back to derives.

`Copy` on `GroundObserver`, `Observation` and `Apsis` is a one-way door — removing it is breaking,
and it forecloses ever adding a non-`Copy` field (a station name on `GroundObserver` is the
obvious candidate). Accepted: they are small numeric records and pass-by-value is how they read.

The window types' derived `Ord` is field-order dependent, and the chronological order is a
documented promise. `time.rs`'s `test_window_ordering_is_chronological` pins it for all four.

## Repo infrastructure

- **Git hooks**: managed by `prek` (`prek.toml`). Pre-commit runs fmt+clippy; pre-push runs test+coverage. Contributors install with `prek install`.
- **Dependencies**: `serde_yaml` (not `serde_yml`) is used for YAML parsing in dev/tests.

### Releasing

Full process in `docs/RELEASING.md`. `release-prepare.yml` (manual dispatch) bumps versions and rolls changelogs via `cargo-release` (`release.toml`), then opens a release PR; merging it triggers `release.yml`, which publishes, tags, and opens a GitHub Release per crate.

**All three crates share `major.minor`; `patch` moves independently.** A `minor`/`major` bump is workspace-wide and zeroes patch everywhere, so alignment is automatic; a `patch` bump may be scoped to one crate. Both workflows assert the invariant, as does `test.yml`.

Points that are deliberate and easy to "fix" wrongly:

- `release.toml`'s changelog pattern is anchored (`(?m)^## \[Unreleased\]$`). Unanchored, it also matches each changelog preamble's prose reference to that heading and trips `exactly = 1`.
- The second replacement block bumps the library README's `sgp4-predict = "…"` install snippet. It uses `min = 0`, not `exactly = 1`, because `pre-release-replacements` run per crate and only one of the three READMEs carries such a line.
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
