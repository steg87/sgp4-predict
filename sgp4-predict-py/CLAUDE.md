# Python bindings (`sgp4-predict-py/`)

## Commands

Run from within `sgp4-predict-py/`:

```bash
make dev    # compile the Rust extension in-place (maturin develop)
make test   # compile + run pytest
make lint   # ruff check --fix + ruff format (fixes in place, like the Rust make lint)
```

To regenerate stubs after Rust API changes, run from the **repo root**:

```bash
PYO3_PYTHON=sgp4-predict-py/.venv/bin/python \
  cargo run --manifest-path sgp4-predict-py/Cargo.toml --bin stub_gen
```

`make stubs` inside `sgp4-predict-py/` fails when `VIRTUAL_ENV` points elsewhere — use the explicit command above instead.

## Type stubs

`python/sgp4_predict/_sgp4_predict/__init__.pyi` is **generated but committed**, because the release workflow only runs `maturin build` on a clean checkout — an ignored stub would simply be absent from every published wheel, leaving everything it declares untyped. `python.yml` regenerates it and fails on a diff, so it cannot drift. `py.typed` sits beside it; without that marker PEP 561 tells type checkers to ignore the stubs entirely.

A `&Bound<'_, PyAny>` parameter would otherwise land in the stub as `typing.Any`, so each one carries `#[gen_stub(override_type(type_repr = …, imports = …))]` naming the Python type it really accepts — `sgp4_predict.IntervalRange`, `sgp4_predict.Area`, `sgp4_predict.LatLonLike`, `builtins.dict`. The resulting import cycle between the two stub files is fine; type checkers resolve stub imports lazily.

`__next__` returns `PyResult<Option<T>>`, but pyo3 turns `Ok(None)` into `StopIteration` rather than yielding it, so each one carries `#[gen_stub(override_return_type(…))]` naming `T`. Without it every `for x in iter:` binds `T | None` and reads as a type error downstream.

Consequently the hand-maintained `sgp4_predict/__init__.pyi` holds **only** what cannot be generated: the `IntervalRange` protocol, the `Interval` class, and the `LatLonLike`/`Area` aliases. Do not redeclare a generated class there — a redeclaration _replaces_ it, silently dropping every docstring the Rust source supplies.

## Tuning knobs

Every method with a `*_with_opts` sibling in the library takes that struct's fields as keyword-only arguments, each `Option` defaulting to `None`. The builders in `predictor.rs` resolve `None` against the library's own `Default`, so a value is never written twice and the bindings cannot drift from the library; the unit tests there pin that and the per-kwarg wiring.

A default's value is not visible from Python. Anything the bindings could show is the _requested_ value, not the one the run uses — the library clamps afterwards (`aoi.rs`'s `step_bounds`, `MIN_POSITIVE_STEP` elsewhere) — so showing it would need the library to resolve its `*Opts` and hand the resolved struct back.

The `Refinement` used by the iterators comes from `Predictor.with_refinement`, not a per-call kwarg.

## Conventions

Angles are plain `float` with `_deg`-suffixed field/arg names, converted to/from the library's `Degrees`/`Radians` at the FFI boundary — the Rust type safety deliberately stops here.

In Rust, `Observer` is the _trait_ and `GroundObserver` the concrete type; in Python the class is also named `GroundObserver`.

## Areas of interest (`area.rs`)

`area.rs` wraps all three shapes and dispatches through a private `AreaKind` enum implementing `Area`. That exists because `AoiIter<'a, A: Area>` is generic and `A` is implicitly `Sized`, so `Box<dyn Area>` does not fit without relaxing the library's bound; the enum keeps the change on the Python side. `AoiIter` then borrows an owned `AreaKind` through `ouroboros`, exactly as `TransitIter` borrows its `GroundObserver`.

`AreaRef<'a>` is the borrowed twin of `AreaKind`, for `detect_aoi`, which does not outlive its argument. All three pyclasses are `frozen`, so `Bound::cast::<Polygon>()?.get()` yields a reference and the vertex vector is never cloned; `extract_area` clones out of an `AreaRef` rather than duplicating the dispatch. Note pyo3 spells this `cast`, not `downcast`. `extract_lat_lon` uses `cast` for that reason plus one more: the tuple form is the documented common case, and only `cast` misses without constructing a Python exception.

Constructors take `&Bound<'_, PyAny>` so a point may be a `LatLon`, a `Geodetic`, or a `(latitude_deg, longitude_deg)` tuple; each is annotated back to `sgp4_predict.LatLonLike` for the stub, as described above.

The cap `max_window_duration` sets is only escapable for an area the track actually leaves — a whole-Earth box has no window end, so it raises whatever the cap is.

Test-sizing gotcha: a _symmetric_ latitude band cannot trip the one-hour default for a LEO satellite, because its two in-band arcs are each at most half an orbit and only exceed an hour by merging (i.e. permanently inside). `tests/test_aoi.py` uses `latitude_band(-90, 60)`, whose windows are ~85 min.
