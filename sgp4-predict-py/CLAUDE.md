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

**Known stub-gen limitation**: pyo3-stub-gen silently drops static methods whose parameters are `&Bound<'_, PyAny>` (e.g. `Elements.from_dict`). Such methods work at runtime but never appear in `_sgp4_predict/__init__.pyi`. If that becomes a problem, override the signature in the hand-maintained `sgp4_predict/__init__.pyi`.

## Tuning knobs

Every method with a `*_with_opts` sibling in the library takes that struct's fields as keyword-only arguments, each `Option` defaulting to `None`. The builders in `predictor.rs` resolve `None` against the library's own `Default`, so a value is never written twice and the bindings cannot drift from the library; the unit tests there pin that and the per-kwarg wiring.

Literal defaults in the `#[pyo3(signature = …)]` were tried and rejected: pyo3-stub-gen and `__text_signature__` both render an expression like `Duration::seconds(60)` as `...`, so the value is invisible in the stub _and_ duplicated in Rust. Instead the four detection iterators carry a copy of their resolved `*Opts` and print it from `__repr__`, which is what makes a default discoverable from Python — and it reports overrides in the same breath. The pytest asserting those repr strings is the only place the default values appear as literals, which is deliberate: it fails loudly when the library changes one.

The `Refinement` used by the iterators still comes from `Predictor.with_refinement`, not a per-call kwarg.

## Conventions

Angles are plain `float` with `_deg`-suffixed field/arg names, converted to/from the library's `Degrees`/`Radians` at the FFI boundary — the Rust type safety deliberately stops here.

In Rust, `Observer` is the _trait_ and `GroundObserver` the concrete type; in Python the class is also named `GroundObserver`.

## Areas of interest (`area.rs`)

`area.rs` wraps all three shapes and dispatches through a private `AreaKind` enum implementing `Area`. That exists because `AoiIter<'a, A: Area>` is generic and `A` is implicitly `Sized`, so `Box<dyn Area>` does not fit without relaxing the library's bound; the enum keeps the change on the Python side. `AoiIter` then borrows an owned `AreaKind` through `ouroboros`, exactly as `TransitIter` borrows its `GroundObserver`.

`AreaRef<'a>` is the borrowed twin of `AreaKind`, for `detect_aoi`, which does not outlive its argument. All three pyclasses are `frozen`, so `Bound::cast::<Polygon>()?.get()` yields a reference and the vertex vector is never cloned; `extract_area` clones out of an `AreaRef` rather than duplicating the dispatch. Note pyo3 spells this `cast`, not `downcast`. `extract_lat_lon` uses `cast` for that reason plus one more: the tuple form is the documented common case, and only `cast` misses without constructing a Python exception.

Constructors take `&Bound<'_, PyAny>` so a point may be a `LatLon`, a `Geodetic`, or a `(latitude_deg, longitude_deg)` tuple. pyo3-stub-gen therefore widens them to `Any`, so `Polygon`/`Rectangle`/`Ellipse` are redeclared in the hand-maintained `sgp4_predict/__init__.pyi` — and a redeclaration there _replaces_ the generated class rather than merging with it, so every member has to be repeated.

The cap `max_window_duration` sets is only escapable for an area the track actually leaves — a whole-Earth box has no window end, so it raises whatever the cap is.

Test-sizing gotcha: a _symmetric_ latitude band cannot trip the one-hour default for a LEO satellite, because its two in-band arcs are each at most half an orbit and only exceed an hour by merging (i.e. permanently inside). `tests/test_aoi.py` uses `latitude_band(-90, 60)`, whose windows are ~85 min.
