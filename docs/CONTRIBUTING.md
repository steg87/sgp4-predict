# Contributing

Contributions are welcome. This document covers the standard workflow for submitting changes.

## Required tooling

| Tool | Purpose | Install |
|---|---|---|
| Rust stable | compiler, rustfmt, clippy | [rustup.rs](https://rustup.rs) |
| cargo-llvm-cov | coverage (pre-push hook) | `cargo install cargo-llvm-cov` |
| prek | git hook runner | [prek docs](https://github.com/blinpete/prek) |
| uv | Python tooling | [docs.astral.sh/uv](https://docs.astral.sh/uv) |
| maturin | build Python bindings | bundled via `uv sync --extra dev` |

## Getting started

1. Fork the repository on GitHub.
2. Clone your fork and create a branch for your change:
   ```
   git checkout -b my-feature
   ```
3. Install the git hooks:
   ```
   prek install
   ```
   This sets up pre-commit hooks (fmt, clippy) and pre-push hooks (test, coverage).
4. Make your changes, then open a pull request against `main`.

## Python bindings setup

The `sgp4-predict-py/` crate contains Python bindings built with [maturin](https://github.com/PyO3/maturin). To work on them:

```bash
cd sgp4-predict-py/
uv sync --extra dev   # create .venv and install dev dependencies
make dev              # compile the Rust extension in-place (maturin develop)
make test             # compile the Rust extension + run pytest
make stubs            # regenerate _sgp4_predict/__init__.pyi after Rust changes
make lint             # ruff check + format
```

`make` targets use `uv run` internally, so no manual venv activation is needed.

## Before submitting

Run the following before pushing:

```bash
make lint    # cargo fmt + clippy (must be clean)
make test    # full test suite
```

CI enforces both. PRs with lint errors or failing tests will not be merged.

## Guidelines

- **Keep PRs focused.** One logical change per PR makes review faster and history easier to read.
- **Add tests** for any new behaviour. The existing tests in `tests/` show the expected style.
- **Don't break the public API** without good reason. If you need to, note it clearly in the PR description.
- **All units are SI.** Positions in metres, velocities in m/s, angles in radians. Don't introduce conversions into the library itself.

## Reporting bugs

Open an issue on GitHub with a minimal reproducer — the TLE, observer coordinates, time range, and the unexpected output.
