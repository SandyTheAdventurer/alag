# Development

ALAG is a Python package with a compiled Rust command, built with [maturin](https://www.maturin.rs). The Rust side uses only the standard library, and the viewer is one HTML file with no dependencies.

```
python/alag/logger.py   # the ALAG class
src/main.rs             # the alag command: server and --export
src/index.html          # the viewer, built into the command
tests/                  # Python tests
```

## Running the tests

```sh
cargo test
pip install pytest psutil matplotlib
PYTHONPATH=python python -m pytest tests
```

## Building wheels

`scripts/build-wheels.sh` builds the source package and the Linux and Windows wheels into `dist/`. It needs only Docker. CI builds the same wheels plus macOS ones on every push.

## Releasing

1. Bump `version` in `pyproject.toml`, `Cargo.toml` and `python/alag/__init__.py`.
2. Commit and push to `main`, and wait for CI to pass.
3. Tag and push: `git tag v0.1.2 && git push origin v0.1.2`.

The tag starts CI, which uploads every wheel to PyPI through trusted publishing, then creates a GitHub release with notes and the same files.

## Docs

These pages are in `docs/` and built with MkDocs. To preview them:

```sh
pip install -r docs/requirements.txt
mkdocs serve
```

Pushing to `main` rebuilds the site and the [live demo](demo.html).
