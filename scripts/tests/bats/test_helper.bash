#!/usr/bin/env bash
# test_helper.bash — shared helpers for BATS tests under scripts/tests/bats/.
# Loaded via `load test_helper` from each .bats file.

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BATS_TEST_FILENAME}")/../../.." && pwd)"
SCRIPTS_DIR="${REPO_ROOT}/scripts"
LIB_DIR="${SCRIPTS_DIR}/lib"
export REPO_ROOT SCRIPTS_DIR LIB_DIR

# Per-test temp dir.
setup() {
  TEST_TMP="${BATS_TEST_TMPDIR:-$(mktemp -d)}"
  export TEST_TMP
  # Ensure deprecation env doesn't leak between tests.
  unset ARTAGON_VSCODE_DEPRECATION_SEEN ARTAGON_VSCODE_DEPRECATION_ACK \
        ARTAGON_VSCODE_DEPRECATION_SUMMARY ARTAGON_VSCODE_DEPRECATION_COUNT
}

teardown() {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN ARTAGON_VSCODE_DEPRECATION_ACK \
        ARTAGON_VSCODE_DEPRECATION_SUMMARY ARTAGON_VSCODE_DEPRECATION_COUNT \
        ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER ARTAGON_VSCODE_BYPASS_PINS \
        ARTAGON_VSCODE_PINS_FILE FAILING_EXTS VSCODE_USER_DIR
  if [[ -n "${TEST_TMP:-}" ]] && [[ "$TEST_TMP" != "${BATS_TEST_TMPDIR:-}" ]]; then
    rm -rf "$TEST_TMP"
  fi
}

# install_code_mock <bin_dir>
# Drops a `code` mock into <bin_dir> and prepends it to PATH. The mock:
#   - records `--install-extension <id>` calls to $CODE_LOG
#   - exits 1 (and logs action=install_failed) for any <id> matching
#     a comma- or newline-separated entry in $FAILING_EXTS
#   - exits 0 otherwise (no real install happens)
# Caller sets FAILING_EXTS in the env before invoking the script under test.
install_code_mock() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  : "${CODE_LOG:=$TEST_TMP/code.log}"
  export CODE_LOG
  cat > "$bin_dir/code" <<'MOCK'
#!/usr/bin/env bash
profile=""
action="open"
ext=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)            profile="$2"; shift 2 ;;
    --install-extension)  action="install"; ext="$2"; shift 2 ;;
    *)                    shift ;;
  esac
done
if [[ "$action" == "install" ]]; then
  base="$(basename "$ext")"
  ext_id="${base%.vsix}"
  fail_list="${FAILING_EXTS:-}"
  fail_list="${fail_list//,/ }"
  for f in $fail_list; do
    if [[ "$f" == "$ext_id" ]]; then
      echo "action=install_failed profile=${profile:-unknown} ext=${ext_id}" >> "${CODE_LOG:-/tmp/code.log}"
      exit 1
    fi
  done
  echo "action=install profile=${profile:-unknown} ext=${ext_id}" >> "${CODE_LOG:-/tmp/code.log}"
fi
exit 0
MOCK
  chmod +x "$bin_dir/code"
  export PATH="$bin_dir:$PATH"
  export VSCODE_EXTENSION_INSTALL_DELAY=0
}

# make_bundle <out_path> [setting_pairs...] -- [extension_ids...]
# Creates a .code-profile fixture with given workbench settings and
# extension list. Settings are "key=value" pairs; values are written
# as JSON strings.
make_bundle() {
  local out="$1"; shift
  local in_settings=1
  local settings_json="{}"
  local exts_json="[]"
  local pairs=() ids=()
  for arg in "$@"; do
    if [[ "$arg" == "--" ]]; then
      in_settings=0; continue
    fi
    if (( in_settings )); then
      pairs+=("$arg")
    else
      ids+=("$arg")
    fi
  done
  if (( ${#pairs[@]} > 0 )); then
    settings_json=$(jq -n '{}')
    for p in "${pairs[@]}"; do
      local k="${p%%=*}" v="${p#*=}"
      settings_json=$(jq --arg k "$k" --arg v "$v" '.[$k] = $v' <<<"$settings_json")
    done
  fi
  if (( ${#ids[@]} > 0 )); then
    exts_json=$(printf '%s\n' "${ids[@]}" | jq -R . | jq -s .)
  fi
  jq -n --argjson s "$settings_json" --argjson e "$exts_json" \
    '{settings: $s, extensions: {enabled: $e}}' > "$out"
}

# import_profile <name> <bundle_path> ...
# Run scripts/import-profile.sh in this test's environment.
import_profile() {
  bash "$SCRIPTS_DIR/import-profile.sh" "$@"
}

# Build a fixture workspace: empty dir under TEST_TMP, optionally
# pre-populated with given files (zero-byte unless content provided).
make_workspace() {
  local name="${1:-ws}"; shift || true
  local dir="$TEST_TMP/$name"
  mkdir -p "$dir"
  for spec in "$@"; do
    case "$spec" in
      *=*)
        local f="${spec%%=*}"
        local content="${spec#*=}"
        mkdir -p "$dir/$(dirname "$f")"
        printf '%s' "$content" > "$dir/$f"
        ;;
      *)
        mkdir -p "$dir/$(dirname "$spec")"
        : > "$dir/$spec"
        ;;
    esac
  done
  echo "$dir"
}

# Skip the test if the named binary isn't on PATH.
have_bin() {
  command -v "$1" >/dev/null 2>&1
}
require_bin() {
  if ! have_bin "$1"; then
    skip "$1 not on PATH"
  fi
}

# Run the detection script. Args after `--` go to the script.
detect() {
  bash "$SCRIPTS_DIR/detect-toolchain.sh" "$@"
}

# Run vspcli (preserving stderr).
vspcli() {
  bash "$SCRIPTS_DIR/vspcli" "$@"
}
