# sgp4-predict

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![Coverage](https://github.com/steg87/sgp4-predict/actions/workflows/coverage.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/coverage.yml)
[![License: MIT](https://img.shields.io/crates/l/sgp4-predict)](LICENSE)

A Rust workspace for SGP4 satellite pass prediction, from low-level propagation to a ready-to-use command-line tool.

## Crates

| Crate | Description | |
|---|---|---|
| [`sgp4-predict`](sgp4-predict/) | Rust library — propagation, transit detection, observations, apsides, illumination | [![Crates.io](https://img.shields.io/crates/v/sgp4-predict)](https://crates.io/crates/sgp4-predict) [![docs.rs](https://img.shields.io/docsrs/sgp4-predict)](https://docs.rs/sgp4-predict) |
| [`sgp4-predict-cli`](sgp4-predict-cli/) | Command-line tool built on the library — tabular output for all prediction modes | [![Crates.io](https://img.shields.io/crates/v/sgp4-predict-cli)](https://crates.io/crates/sgp4-predict-cli) |
| [`sgp4-predict-py`](sgp4-predict-py/) | Python bindings via PyO3/Maturin | [![PyPI](https://img.shields.io/pypi/v/sgp4-predict)](https://pypi.org/project/sgp4-predict/) |

## Benchmarks and validation

Cross-validation runs the library against [pypredict](https://github.com/nsat/pypredict) and [Skyfield](https://rhodesmill.org/skyfield/) reference data.

```bash
make validation   # cross-validate against pypredict/skyfield (downloads de421.bsp ~17 MB on first run)
make benchmark    # Rust vs pypredict Monte Carlo throughput benchmark
```

## Contributing

See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).
