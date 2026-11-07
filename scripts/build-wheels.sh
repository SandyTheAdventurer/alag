#!/usr/bin/env bash
# Builds the source package and wheels for Linux (x86-64, ARM) and Windows
# into dist/, using Docker only. macOS isn't built: it needs Apple's SDK.
#
# Usage: scripts/build-wheels.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# CentOS 7 with glibc 2.17, so the Linux wheels run on old cluster systems.
IMAGE=ghcr.io/pyo3/maturin:v1.15.0
ZIG=0.15.2

rm -rf dist
docker run --rm \
  -v "$PWD":/io -w /io \
  -e OWNER="$(id -u):$(id -g)" \
  -e CARGO_TARGET_DIR=/tmp/target \
  --entrypoint bash "$IMAGE" -euo pipefail -c "
    pip install --quiet --root-user-action=ignore ziglang==$ZIG
    rustup target add --toolchain stable aarch64-unknown-linux-gnu x86_64-pc-windows-gnu >/dev/null
    maturin sdist --out dist
    maturin build --release --out dist --compatibility manylinux2014
    maturin build --release --out dist --compatibility manylinux2014 --zig --target aarch64-unknown-linux-gnu
    maturin build --release --out dist --zig --target x86_64-pc-windows-gnu
    chown -R \$OWNER dist
  "
ls -l dist
