#!/usr/bin/env bash
# Repository-native quality gate for fast-image-converter.
#
# Runs the same checks the CI workflow (.github/workflows/ci.yml)
# runs: format, strict Clippy, full test suite, release build, and
# a RustSec-compatible advisory scan. Exit non-zero on any failure.
#
# Usage:
#   scripts/check.sh           # full gate
#   scripts/check.sh format    # only the formatter check
#   scripts/check.sh clippy    # only strict Clippy
#   scripts/check.sh test      # only the test suite
#   scripts/check.sh release   # only the release build
#   scripts/check.sh audit     # only the advisory scan
#
# The advisory step uses `cargo audit` from the
# `RustSecurity/rustsec` project. `cargo audit` is installed as a
# standalone binary via `cargo install --locked cargo-audit`; it is
# deliberately NOT a Cargo.toml dev-dependency so the release
# binary stays free of audit-only build artefacts.
#
# Required host tools: rustup with the pinned toolchain in
# `rust-toolchain.toml`, libwebp-dev, libheif-dev (>= 1.21),
# libde265-dev, dav1d-dev, pkg-config, a C toolchain.

set -euo pipefail

step() {
  printf '\n=== %s ===\n' "$1"
}

err() {
  printf 'check.sh: %s\n' "$1" >&2
  exit 1
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "required tool '$1' not on PATH; install it and retry"
  fi
}

run_format() {
  step "cargo fmt --all -- --check"
  cargo fmt --all -- --check
}

run_clippy() {
  step "cargo clippy --all-targets --all-features -- -D warnings"
  cargo clippy --all-targets --all-features -- -D warnings
}

run_test() {
  step "cargo test --all-targets --all-features"
  cargo test --all-targets --all-features
}

run_release() {
  step "cargo build --release"
  cargo build --release
}

run_audit() {
  step "cargo audit --deny warnings"
  if ! command -v cargo-audit >/dev/null 2>&1; then
    err "cargo-audit not installed; run 'cargo install --locked cargo-audit' first"
  fi
  cargo audit --deny warnings
}

target="${1:-all}"

case "$target" in
  all)
    require_tool cargo
    require_tool pkg-config
    run_format
    run_clippy
    run_test
    run_release
    run_audit
    ;;
  format)   require_tool cargo; run_format ;;
  clippy)   require_tool cargo; run_clippy ;;
  test)     require_tool cargo; run_test ;;
  release)  require_tool cargo; run_release ;;
  audit)
    require_tool cargo
    require_tool cargo-audit
    run_audit
    ;;
  *)
    err "unknown target '$target'; expected: all|format|clippy|test|release|audit"
    ;;
esac

step "ok"
