# Python

A Python binding is provided for sgp4-predict crate to allow typed usage of the library in Python environments.

## Local Development

Create and activate the local virtual environment.

```sh
uv sync --extra dev
source .venv/bin/python
```

Run local testing with `make test`.

## Build Stubs

The .pyi stub files provide type checking for the _sgp4_predict.so compiled Rust binary. They are not committed to the Git repo but can be generated from code on demand with `make stubs`.

The are built and included with the PyPI release in CI but developers may with to generate them locally.