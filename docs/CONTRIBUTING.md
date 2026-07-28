# Contributing

Contributions are welcome.

## Tooling

| Tool | Purpose | Install |
|---|---|---|
| Rust stable | compiler, rustfmt, clippy | [rustup.rs](https://rustup.rs) |
| cargo-llvm-cov | coverage (pre-push hook) | `cargo install cargo-llvm-cov` |
| prek | git hook runner | [prek docs](https://github.com/blinpete/prek) |
| uv | Python tooling | [docs.astral.sh/uv](https://docs.astral.sh/uv) |

## Getting started

1. Fork the repository, clone your fork, and branch off `main`.
2. Install the git hooks with `prek install` — this wires up pre-commit (fmt, clippy) and pre-push
   (test, coverage).
3. Make your change and open a pull request against `main`.

Before pushing:

```bash
make lint    # cargo fmt + clippy
make test    # full test suite, including doctests
```

CI enforces both.

## Python bindings

The `sgp4-predict-py/` crate is built with [maturin](https://github.com/PyO3/maturin):

```bash
cd sgp4-predict-py/
uv sync --extra dev   # create .venv and install dev dependencies
make dev              # compile the Rust extension in-place
make test             # compile + run pytest
make lint             # ruff check --fix + ruff format
```

`make` targets use `uv run`, so no venv activation is needed.

After changing the Rust API, regenerate the stubs from the repository root:

```bash
PYO3_PYTHON=sgp4-predict-py/.venv/bin/python \
  cargo run --manifest-path sgp4-predict-py/Cargo.toml --bin stub_gen
```

## Guidelines

- **Keep PRs focused.** One logical change per PR.
- **Add tests** for new behaviour; `tests/` shows the expected style.
- **Don't break the public API** without saying so clearly in the PR description.
- **Keep units SI** inside the library — metres, m/s. Conversions belong at the boundary.
- **Add a changelog entry** under `## [Unreleased]` in the affected crate's `CHANGELOG.md`.

## Reporting bugs

Open an issue with a minimal reproducer: the TLE, observer coordinates, time range, and the
unexpected output.

## Releasing

Maintainers only, and entirely through GitHub Actions — see [RELEASING.md](RELEASING.md).
