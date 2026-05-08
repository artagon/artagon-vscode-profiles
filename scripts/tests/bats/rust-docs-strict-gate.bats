#!/usr/bin/env bats
# Strict rustdoc gate — verifies that `RUSTDOCFLAGS="-D warnings" cargo doc`
# fails on broken intra-doc links and passes on clean fixtures.

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  if ! command -v cargo >/dev/null 2>&1; then
    skip "cargo not on PATH"
  fi
}

@test "strict-docs: clean fixture passes" {
  fixture="$REPO_ROOT/scripts/tests/bats/fixtures/rust-clean-doc"
  [ -d "$fixture" ] || skip "fixture missing: $fixture"

  build_dir=$(mktemp -d)
  cd "$fixture"
  CARGO_TARGET_DIR="$build_dir" \
  RUSTDOCFLAGS="-D warnings" \
    run cargo doc --workspace --all-features --no-deps --quiet
  rm -rf "$build_dir"
  [ "$status" -eq 0 ]
}

@test "strict-docs: broken intra-doc link fails the gate" {
  fixture="$REPO_ROOT/scripts/tests/bats/fixtures/rust-broken-doc"
  [ -d "$fixture" ] || skip "fixture missing: $fixture"

  build_dir=$(mktemp -d)
  cd "$fixture"
  CARGO_TARGET_DIR="$build_dir" \
  RUSTDOCFLAGS="-D warnings" \
    run cargo doc --workspace --all-features --no-deps --quiet
  rm -rf "$build_dir"
  [ "$status" -ne 0 ]
}
