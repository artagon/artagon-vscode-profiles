#!/usr/bin/env bats
# End-to-end coverage for scripts/import-profile.sh:
#   - profile-graceful-defaults: theme/icon-theme strip on failed install
#   - harden-extension-supply-chain: SHA256SUMS verification
#   - harden-extension-supply-chain: publisher allowlist gate
#   - harden-extension-supply-chain: VSIX pin lookup + bypass
# Sequential within file: FAILING_EXTS is a process env var; parallel
# tests within this file would race the shared `code` mock's reads.
# bats file_tags=no-parallel

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  TEST_TMP="${BATS_TEST_TMPDIR}"
  export TEST_TMP
  install_code_mock "$TEST_TMP/bin"
  export VSCODE_USER_DIR="$TEST_TMP/vscode"
  mkdir -p "$VSCODE_USER_DIR"
}

# Resolve the imported profile's settings.json path (UUID is generated
# per-import; use the only profile dir that exists).
profile_settings() {
  local prof_dir
  prof_dir=$(find "$VSCODE_USER_DIR/profiles" -mindepth 1 -maxdepth 1 -type d | head -n 1)
  [ -n "$prof_dir" ] || return 1
  printf '%s\n' "$prof_dir/settings.json"
}

# ---------------------------------------------------------------------------
# profile-graceful-defaults — theme/icon-theme rewrite on failed install
# ---------------------------------------------------------------------------

@test "graceful-defaults: strips workbench.colorTheme when its extension fails" {
  bundle="$TEST_TMP/with-tokyo-night.code-profile"
  make_bundle "$bundle" "workbench.colorTheme=Tokyo Night" -- enkia.tokyo-night github.copilot
  FAILING_EXTS="enkia.tokyo-night" run import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  settings=$(profile_settings)
  run jq -e 'has("workbench.colorTheme")' "$settings"
  [ "$status" -eq 1 ]
}

@test "graceful-defaults: strips iconTheme but preserves colorTheme when only icon-theme fails" {
  bundle="$TEST_TMP/with-both.code-profile"
  make_bundle "$bundle" \
    "workbench.colorTheme=Tokyo Night" \
    "workbench.iconTheme=material-icon-theme" \
    -- enkia.tokyo-night pkief.material-icon-theme
  FAILING_EXTS="pkief.material-icon-theme" run import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  settings=$(profile_settings)
  run jq -e 'has("workbench.iconTheme")' "$settings"
  [ "$status" -eq 1 ]
  run jq -re '."workbench.colorTheme"' "$settings"
  [ "$status" -eq 0 ]
  [ "$output" = "Tokyo Night" ]
}

@test "graceful-defaults: preserves built-in theme name when an unrelated extension fails" {
  bundle="$TEST_TMP/with-builtin.code-profile"
  make_bundle "$bundle" \
    "workbench.colorTheme=Default Dark Modern" \
    -- github.copilot enkia.tokyo-night
  FAILING_EXTS="github.copilot" run import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  settings=$(profile_settings)
  run jq -re '."workbench.colorTheme"' "$settings"
  [ "$output" = "Default Dark Modern" ]
}

@test "graceful-defaults: byte-identical settings on the happy path" {
  bundle="$TEST_TMP/happy.code-profile"
  make_bundle "$bundle" \
    "workbench.colorTheme=Tokyo Night" \
    -- enkia.tokyo-night
  FAILING_EXTS="" run import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  settings=$(profile_settings)
  run diff <(jq -S '.settings' "$bundle") <(jq -S . "$settings")
  [ "$status" -eq 0 ]
}

@test "graceful-defaults: strips theme when value is an explicit publisher.name extension ID" {
  bundle="$TEST_TMP/explicit-id.code-profile"
  make_bundle "$bundle" \
    "workbench.colorTheme=enkia.tokyo-night" \
    -- enkia.tokyo-night
  FAILING_EXTS="enkia.tokyo-night" run import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  settings=$(profile_settings)
  run jq -e 'has("workbench.colorTheme")' "$settings"
  [ "$status" -eq 1 ]
}

# ---------------------------------------------------------------------------
# harden-extension-supply-chain — SHA256SUMS verification
# ---------------------------------------------------------------------------

@test "sha256sums: bundle hash matches manifest -> import proceeds silently" {
  bundle_dir="$TEST_TMP/exports"
  mkdir -p "$bundle_dir"
  bundle="$bundle_dir/web-astro.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  hash=$( { command -v sha256sum >/dev/null && sha256sum "$bundle"; } || shasum -a 256 "$bundle" | head -c 64; )
  hash=$(printf '%s' "$hash" | awk '{print $1}')
  printf '%s  %s\n' "$hash" "web-astro.code-profile" > "$bundle_dir/SHA256SUMS"
  run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  [[ "$stderr" != *"SHA256SUMS not found"* ]]
  [[ "$stderr" != *"SHA256 mismatch"* ]]
}

@test "sha256sums: bundle hash mismatches manifest -> hard fail before any write" {
  bundle_dir="$TEST_TMP/exports"
  mkdir -p "$bundle_dir"
  bundle="$bundle_dir/web-astro.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "web-astro.code-profile" > "$bundle_dir/SHA256SUMS"
  run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  [[ "$stderr" == *"SHA256 mismatch"* ]]
  # No write should have happened — settings dir absent.
  [ ! -d "$VSCODE_USER_DIR/profiles" ]
}

@test "sha256sums: missing manifest -> warn-and-proceed" {
  bundle_dir="$TEST_TMP/exports"
  mkdir -p "$bundle_dir"
  bundle="$bundle_dir/web-astro.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"SHA256SUMS not found"* ]]
}

@test "sha256sums: bundle not listed in manifest -> warn-and-proceed" {
  bundle_dir="$TEST_TMP/exports"
  mkdir -p "$bundle_dir"
  bundle="$bundle_dir/web-astro.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "other.code-profile" > "$bundle_dir/SHA256SUMS"
  run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"not listed in SHA256SUMS"* ]]
}

# ---------------------------------------------------------------------------
# harden-extension-supply-chain — publisher allowlist
# ---------------------------------------------------------------------------

@test "publisher-allowlist: rejects bundle with unknown publisher" {
  bundle="$TEST_TMP/typosquat.code-profile"
  make_bundle "$bundle" -- evilco.bad-actor
  run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 1 ]
  [[ "$stderr" == *"publisher evilco not in allowlist"* ]]
}

@test "publisher-allowlist: ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 bypasses with stderr warning" {
  bundle="$TEST_TMP/exploratory.code-profile"
  make_bundle "$bundle" -- evilco.bad-actor
  ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"unknown publisher evilco"* ]]
}

# ---------------------------------------------------------------------------
# harden-extension-supply-chain — VSIX pinning
# ---------------------------------------------------------------------------

@test "vsix-pin: no pin entry -> falls through to live marketplace path" {
  bundle="$TEST_TMP/unpinned.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  pins="$TEST_TMP/empty-pins.json"
  printf '%s\n' '{"version":1,"pins":{}}' > "$pins"
  ARTAGON_VSCODE_PINS_FILE="$pins" run import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  # The mock recorded the install via live path (extension id, not vsix path).
  grep -q 'action=install profile=test-profile ext=enkia.tokyo-night' "$CODE_LOG"
}

@test "vsix-pin: ARTAGON_VSCODE_BYPASS_PINS=1 skips pin lookup with stderr notice" {
  bundle="$TEST_TMP/bypass.code-profile"
  make_bundle "$bundle" -- enkia.tokyo-night
  pins="$TEST_TMP/with-bad-pin.json"
  jq -n '{version:1, pins:{"enkia.tokyo-night":{version:"1.0.0", sha256:"deadbeef", vsix_url:"https://invalid.localhost/no.vsix"}}}' > "$pins"
  ARTAGON_VSCODE_PINS_FILE="$pins" ARTAGON_VSCODE_BYPASS_PINS=1 run --separate-stderr import_profile test-profile "$bundle"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"bypassing pin manifest"* ]]
  # Live path was used (no fetch attempt for the VSIX).
  grep -q 'action=install profile=test-profile ext=enkia.tokyo-night' "$CODE_LOG"
}
