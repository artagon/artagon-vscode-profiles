#!/usr/bin/env bats
# Strict rustdoc gate — verifies that `RUSTDOCFLAGS="-D warnings" cargo doc`
# fails with a real broken-link rustdoc diagnostic and passes on clean fixtures.

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  setup_hermetic_env
  if ! command -v cargo >/dev/null 2>&1; then
    if [[ "${CI:-}" == "true" ]]; then
      echo "ERROR: cargo is required in CI" >&2
      return 1
    fi
    skip "cargo not on PATH"
  fi
}

@test "strict-docs: clean fixture passes" {
  fixture="${REPO_ROOT}/scripts/tests/bats/fixtures/rust-clean-doc"
  [[ -d "${fixture}" ]] || fail "fixture missing: ${fixture}"

  build_dir="${TEST_TMP}/rust-clean-target"
  mkdir -p "${build_dir}"
  cd "${fixture}"
  CARGO_TARGET_DIR="${build_dir}" \
  RUSTDOCFLAGS="-D warnings" \
    run cargo doc --workspace --all-features --no-deps --quiet
  [[ "${status}" -eq 0 ]]
}

@test "strict-docs: broken intra-doc link fails with explicit rustdoc diagnostic" {
  fixture="${REPO_ROOT}/scripts/tests/bats/fixtures/rust-broken-doc"
  [[ -d "${fixture}" ]] || fail "fixture missing: ${fixture}"

  build_dir="${TEST_TMP}/rust-broken-target"
  mkdir -p "${build_dir}"
  cd "${fixture}"
  CARGO_TARGET_DIR="${build_dir}" \
  RUSTDOCFLAGS="-D warnings" \
    run cargo doc --workspace --all-features --no-deps
  [[ "${status}" -ne 0 ]]
  # Verify failure came from rustdoc intra-doc link check (not toolchain/offline error).
  [[ "${output}" == *"unresolved link"* || "${output}" == *"broken_intra_doc_links"* ]]
}
